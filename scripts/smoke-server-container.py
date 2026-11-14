#!/usr/bin/env python3
"""Isolated CPU/GPU API smoke; defaults to CPU and never uses a microphone."""

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import http.client
import io
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time
import urllib.request
import uuid
import wave


def docker(*args):
    return subprocess.run(["docker", *args], check=True, text=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", required=True)
    parser.add_argument("--device", default="cpu", help="cpu, vulkan, or cuda:N; GPU evidence currently uses NVIDIA")
    parser.add_argument("--gpu-device", default="nvidia.com/gpu=all", help="Docker GPU/CDI mapping for this isolated test")
    parser.add_argument("--model", default="whisper:tiny", help="Managed Whisper ID for the English public fixture")
    parser.add_argument("--models-dir", type=Path, help="Reuse this isolated model directory instead of downloading into temporary storage")
    parser.add_argument("--crash-worker", action="store_true", help="Kill only this container's GPU worker during a request")
    args = parser.parse_args()
    if not re.fullmatch(r"cpu|vulkan|cuda:(?:0|[1-9][0-9]*)", args.device):
        parser.error("--device must be cpu, vulkan or a canonical cuda:N ordinal")
    if args.crash_worker and args.device == "cpu":
        parser.error("--crash-worker requires a GPU backend")
    if not re.fullmatch(r"whisper:[a-z0-9_]{1,64}", args.model):
        parser.error("--model must be a managed Whisper ID")
    image = args.image
    if docker("image", "inspect", "--format", "{{.Config.User}}", image).strip() != "10001:10001":
        raise RuntimeError("The packaged image does not default to the Fono user")
    if os.getuid() == 0:
        raise RuntimeError("Run the smoke as an unprivileged Docker user")
    container = "fono-cpu-smoke-" + uuid.uuid4().hex
    with tempfile.TemporaryDirectory(prefix="fono-cpu-smoke-") as temporary:
        root = Path(temporary)
        (root / "models").mkdir()
        (root / "credentials").mkdir()
        (root / "cache").mkdir()
        (root / "server.toml").write_text('''[server]
bind = "0.0.0.0:17832"
allowed_hosts = ["127.0.0.1"]
trusted_private_network = true
tokens_file = "/qa/credentials/tokens"
[rate_limit]
requests_per_minute = 6000
burst = 256
[models]
dir = "/qa/models"
default = "MODEL"
preload = ["MODEL"]
[[models.placement]]
model = "MODEL"
device = "DEVICE"
residency = "resident"
'''.replace("DEVICE", args.device).replace("MODEL", args.model), encoding="utf-8")
        run = ["run", "--rm", "--user", f"{os.getuid()}:{os.getgid()}",
               "--mount", f"type=bind,src={root},dst=/qa",
               "-e", "FONO_CONFIG=/qa/server.toml", "-e", "XDG_CACHE_HOME=/qa/cache"]
        if args.device != "cpu":
            run.extend(["--device", args.gpu_device])
        if args.models_dir:
            models_dir = args.models_dir.resolve(strict=True)
            if not models_dir.is_dir() or "," in str(models_dir):
                parser.error("--models-dir must be a real directory without commas")
            run.extend(["--mount", f"type=bind,src={models_dir},dst=/qa/models"])
        docker(*run, image, "models", "pull", args.model)
        secret = docker(*run, image, "token", "create", "ci-smoke").strip().splitlines()[-1]
        if not secret.startswith("fono_"):
            raise RuntimeError("Token creation returned an unexpected format")
        diagnosis = json.loads(docker(*run, image, "doctor"))
        if not diagnosis["ready"]:
            raise RuntimeError("Installation diagnostics failed")
        probe = diagnosis.get("self_test", {})
        if (probe.get("status"), probe.get("model"), probe.get("device")) != (
                "passed", args.model, args.device):
            raise RuntimeError("Doctor did not verify speech on the configured model/backend")
        # Upstream JFK fixture pinned to whisper.cpp v1.8.3.
        url = "https://raw.githubusercontent.com/ggml-org/whisper.cpp/2eeeba56e9edd762b4b38467bab96c2517163158/samples/jfk.wav"
        with urllib.request.urlopen(url, timeout=60) as response:
            speech = response.read(2_000_001)
        if hashlib.sha256(speech).hexdigest() != "59dfb9a4acb36fe2a2affc14bacbee2920ff435cb13cc314a08c13f66ba7860e":
            raise RuntimeError("Public speech fixture SHA-256 mismatch")
        try:
            docker(*run, "--name", container, "--detach",
                   "--publish", "127.0.0.1::17832", image, "serve")
            port = int(docker("port", container, "17832/tcp").strip().rsplit(":", 1)[1])
            deadline = time.monotonic() + 120
            while True:
                try:
                    status, _ = request(port, "GET", "/healthz")
                    if status == 200:
                        break
                except (OSError, http.client.HTTPException):
                    pass
                if time.monotonic() >= deadline:
                    raise RuntimeError("Container never became ready")
                time.sleep(0.5)
            status, _ = request(port, "GET", "/v1/capabilities")
            if status != 401:
                raise RuntimeError("Discovery unexpectedly allowed anonymous access")
            status, payload = request(port, "GET", "/v1/capabilities",
                                      headers={"Authorization": "Bearer " + secret})
            capabilities = json.loads(payload)
            if status != 200 or capabilities.get("api_version") != 2 or not any(
                    device["id"] == args.device for device in capabilities["devices"]):
                raise RuntimeError("Discovery did not report the selected device")
            status, payload = request(port, "POST", "/v1/recognize?language=en&timestamps=segments",
                                      speech, {"Authorization": "Bearer " + secret, "Content-Type": "audio/wav"})
            result = json.loads(payload)
            if status != 200 or result.get("device") != args.device or "country" not in result.get("text", "").lower():
                raise RuntimeError("Recognition smoke failed")
            summary = {"healthz": 200, "unauthorized": 401, "capabilities": 200, "recognize": 200,
                       "device": result["device"], "audio_ms": result["timings"]["audio_ms"],
                       "doctor": diagnosis["ready"], "doctor_probe": probe["status"], "model": args.model}
            if args.device != "cpu":
                worker_name = "fono-stt-vulkan" if args.device == "vulkan" else "fono-stt-cuda-w"
                processes = docker("top", container, "-eo", "pid,comm")
                pids = {int(fields[0]) for line in processes.splitlines()[1:]
                        if len(fields := line.split()) == 2 and fields[1].startswith(worker_name)}
                gpu_processes = subprocess.run(["nvidia-smi", "pmon", "-c", "1", "-s", "m"],
                                               check=True, text=True, capture_output=True).stdout
                workers = [fields for line in gpu_processes.splitlines()
                           if len(fields := line.split()) >= 4 and fields[1].isdigit() and int(fields[1]) in pids]
                if len(workers) != 1 or not workers[0][3].isdigit():
                    raise RuntimeError("NVIDIA driver did not observe the Fono worker")
                summary["gpu_worker_memory_mb"] = int(workers[0][3])
                summary["gpu_evidence"] = "nvidia-smi pmon matched to this container's worker PID"
            if args.crash_worker:
                crash_and_recover(port, container, secret, speech, args.device)
                summary["worker_crash"] = 502
                summary["worker_recovery"] = 200
            print(json.dumps(summary))
        finally:
            subprocess.run(["docker", "rm", "--force", "--volumes", container], check=False,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def crash_and_recover(port, container, secret, speech, device):
    with wave.open(io.BytesIO(speech)) as source:
        if (source.getnchannels(), source.getsampwidth(), source.getframerate()) != (1, 2, 16000):
            raise RuntimeError("Unexpected speech fixture format")
        pcm = source.readframes(source.getnframes()) * 20
    repeated = io.BytesIO()
    with wave.open(repeated, "wb") as target:
        target.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
        target.writeframes(pcm)
    headers = {"Authorization": "Bearer " + secret, "Content-Type": "audio/wav"}
    with ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(request, port, "POST", "/v1/recognize?language=en", repeated.getvalue(), headers)
        deadline = time.monotonic() + 30
        while not future.done():
            status, payload = request(port, "GET", "/v1/health", headers={"Authorization": "Bearer " + secret})
            if status == 200 and any(model["state"] == "busy" and model["device"] == device
                                     for model in json.loads(payload)["models"]):
                # The container has its own PID namespace. No host PID is killed.
                docker("exec", container, "sh", "-c", '''
found=0
for directory in /proc/[0-9]*; do
  read -r name < "$directory/comm" || continue
  if [ "$name" = "$1" ]; then
    kill -9 "${directory##*/}"
    found=1
  fi
done
test "$found" = 1
''', "fono-smoke", "fono-stt-vulkan" if device == "vulkan" else "fono-stt-cuda-w")
                break
            if time.monotonic() >= deadline:
                raise RuntimeError("GPU request never became busy")
            time.sleep(0.01)
        else:
            raise RuntimeError("GPU request ended before the crash probe")
        status, payload = future.result(timeout=90)
        if status != 502 or json.loads(payload).get("error", {}).get("code") != "worker_failed":
            raise RuntimeError("Worker crash did not return 502 worker_failed")
    status, payload = request(port, "POST", "/v1/recognize?language=en", speech, headers)
    if status != 200 or json.loads(payload).get("device") != device:
        raise RuntimeError("The next request did not recover the selected GPU worker")


def request(port, method, path, body=None, headers=None):
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=90)
    try:
        connection.request(method, path, body=body, headers=headers or {})
        response = connection.getresponse()
        return response.status, response.read(2_000_000)
    finally:
        connection.close()


if __name__ == "__main__":
    main()
