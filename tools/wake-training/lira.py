"""One bounded Russian Lira candidate. Synthetic/public train; private voice stays dev-only."""
from __future__ import annotations
import argparse
import collections
import importlib.metadata
import json
from pathlib import Path
import re
import sys
import time
import wave

import candidate as common

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "wake-bench"))
import recordings

PHRASE_ID = "privet_lira"
PHRASE = "Привет, Лира"
PUBLIC_MANIFEST_SHA256 = "9135c5d2c751146ce229dbedac783d89bd584590dd3840342567f12d57360241"
EPOCHS = 32


def normalized(text):
    return " ".join(re.findall(r"\w+", text.casefold()))


def training_sources(path):
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    if (data.get("schema") != "fono-wake-synthetic-candidate@1"
            or data.get("phrase_id") != PHRASE_ID
            or data.get("target_phrases") != {"ru": PHRASE}
            or data.get("owner_voice_used") is not False
            or data.get("reference_audio_used") is not False):
        raise ValueError("explicit synthetic-only Lira training identity required")
    entries = data.get("entries", [])
    if not 1 <= len(entries) <= 200:
        raise ValueError("invalid bounded training set")
    seen, sources = set(), []
    for entry in entries:
        name = entry["wav"]
        if (Path(name).name != name or "/" in name or "\\" in name or name in seen
                or entry.get("origin") != "synthetic" or entry.get("split") != "train"
                or entry.get("language") != "ru" or type(entry.get("positive")) is not bool
                or (normalized(entry.get("text", "")) == normalized(PHRASE)) != entry["positive"]):
            raise ValueError("training entry identity, label or split mismatch")
        audio = Path(path).parent / name
        if common.digest(audio) != entry["sha256"]:
            raise ValueError("synthetic WAV hash mismatch")
        pcm = common.read_pcm(audio)
        if len(pcm) != entry["samples"]:
            raise ValueError("synthetic length mismatch")
        seen.add(name)
        sources.append((entry, pcm))
    if {e["positive"] for e, _ in sources} != {True, False}:
        raise ValueError("positive and negative synthetic sources required")
    return sources


def public_sources(path):
    if common.digest(path) != PUBLIC_MANIFEST_SHA256:
        raise ValueError("public training selection changed")
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    import numpy as np
    result, seen = [], set()
    for entry in data["entries"]:
        source = entry.get("source", {})
        physical_hash = entry.get("replay_sha256", entry["sha256"])
        if (source.get("dataset") != "google/fleurs"
                or source.get("revision") != "437d9e667fa3a2a08a1957f810bf2c3b5050d221"
                or entry["language"] not in ("ru", "en") or physical_hash in seen
                or normalized(PHRASE) in normalized(entry["reference"])):
            raise ValueError("invalid public negative identity")
        audio = Path(entry["path"])
        if not audio.is_absolute():
            audio = Path(path).resolve().parent / audio
        if audio.stat().st_size > 90 * common.RATE * 2 + 4096 or common.digest(audio) != physical_hash:
            raise ValueError("public WAV hash or size mismatch")
        with wave.open(str(audio), "rb") as wav:
            if (wav.getnchannels(), wav.getsampwidth(), wav.getframerate(), wav.getcomptype()) != (1, 2, common.RATE, "NONE"):
                raise ValueError("invalid public PCM format")
            count = wav.getnframes()
            raw = wav.readframes(count)
        # The existing pinned replay manifest retains the original sample count;
        # the verified resampler output can be one sample shorter.
        allowed_delta = 1 if "replay_sha256" in entry else 0
        if abs(count - entry["samples"]) > allowed_delta or len(raw) != count * 2 or count < 320:
            raise ValueError("public WAV length mismatch")
        result.append(np.frombuffer(raw, dtype="<i2").astype(np.float32) / 32768)
        seen.add(physical_hash)
    if len(result) != 20:
        raise ValueError("expected the frozen 20-clip public selection")
    return result


def train(args):
    if not 1 <= args.max_seconds <= 180:
        raise ValueError("training budget must be at most 180 seconds")
    import numpy as np
    import torch
    import onnx
    import onnxruntime as ort
    synthetic, public = training_sources(args.training_manifest), public_sources(args.public_speech)
    output = Path(args.output_dir)
    output.mkdir(parents=True, exist_ok=False)
    torch.set_num_threads(4)
    torch.manual_seed(common.SEED)
    torch.use_deterministic_algorithms(True)
    rng = np.random.default_rng(common.SEED)
    config = {"schema": "fono-wake-lira-candidate@1", "phrase_id": PHRASE_ID, "phrase": PHRASE,
              "seed": common.SEED, "epochs": EPOCHS, "threshold": common.THRESHOLD,
              "window_samples": common.WINDOW, "stride_samples": common.STRIDE,
              "tail_samples": common.TAIL, "cooldown_samples": common.COOLDOWN,
              "training_seconds_limit": args.max_seconds, "causal_prefix_training": True,
              "architecture": "conv8-16-24,pool2x3,adaptive1x10,fc32,fc1,sigmoid",
              "source_sha256": common.digest(__file__), "frontend_sha256": common.digest(common.__file__),
              "recording_loader_sha256": common.digest(recordings.__file__),
              "synthetic_manifest_sha256": common.digest(args.training_manifest),
              "public_manifest_sha256": common.digest(args.public_speech),
              "public_original_split": "test", "wake_split": "train",
              "excluded_from_future_wake_test": "all google/fleurs data; speaker overlap unknown",
              "owner_voice_used_for_training": False, "qualified": False,
              "versions": {n: importlib.metadata.version(n) for n in ("numpy", "scipy", "torch", "onnx", "onnxruntime")}}
    common.write_json(output / "config.json", config)
    started = time.monotonic()

    def background(prefix=False):
        pcm = public[int(rng.integers(len(public)))]
        if prefix:
            length = int(rng.integers(320, min(len(pcm), common.WINDOW) + 1))
            return np.pad(pcm[:length], (common.WINDOW - length, 0))
        if len(pcm) >= common.WINDOW:
            begin = int(rng.integers(len(pcm) - common.WINDOW + 1))
            return pcm[begin:begin + common.WINDOW].copy()
        return np.pad(pcm, (0, common.WINDOW - len(pcm)))

    values, labels, excluded = [], [], 0
    for entry, pcm in synthetic:
        if len(pcm) > common.WINDOW:
            excluded += 1
            continue
        for index in range(48 if entry["positive"] else 12):
            if index % 2 == 0:
                speed = rng.uniform(0.82, 1.18)
                stretched = np.interp(np.arange(0, len(pcm), speed), np.arange(len(pcm)), pcm).astype(np.float32)
                if len(stretched) > common.WINDOW:
                    continue
                value = np.zeros(common.WINDOW, dtype=np.float32)
                position = int(rng.integers(common.WINDOW - len(stretched) + 1))
                value[position:position + len(stretched)] = stretched * rng.uniform(0.4, 1.5)
            else:
                value = common.augmented(pcm, rng)
            if value is None:
                continue
            if index % 4 == 1:
                value += background() * rng.uniform(0.05, 0.3)
            values.append(np.clip(value, -1, 1))
            labels.append(float(entry["positive"]))
    for index in range(768):
        values.append(np.clip(background(prefix=index % 2 == 0) * rng.uniform(0.35, 1.5), -1, 1))
        labels.append(0.0)
    for _ in range(48):
        values.append(rng.normal(0, rng.uniform(0, 0.01), common.WINDOW).astype(np.float32))
        labels.append(0.0)
    if set(labels) != {0.0, 1.0}:
        raise ValueError("duration exclusions removed a required training class")
    features = torch.cat([common.frontend(np.stack(values[i:i + common.BATCH])) for i in range(0, len(values), common.BATCH)])
    del values
    targets = torch.tensor(labels).reshape(-1, 1)
    positives, negatives = int(targets.sum()), len(targets) - int(targets.sum())
    model = common.network()
    optimizer = torch.optim.AdamW(model.parameters(), lr=0.001, weight_decay=0.01)
    loss_fn = torch.nn.BCEWithLogitsLoss(pos_weight=torch.tensor([negatives / positives]))
    history = []
    for epoch in range(EPOCHS):
        model.train()
        total = 0.0
        for indices in torch.randperm(len(features)).split(common.BATCH):
            if time.monotonic() - started > args.max_seconds:
                raise TimeoutError("single training budget exhausted; no accepted artifact")
            optimizer.zero_grad(set_to_none=True)
            loss = loss_fn(model(features[indices]), targets[indices])
            if not torch.isfinite(loss):
                raise ValueError("nonfinite loss")
            loss.backward()
            optimizer.step()
            total += float(loss.detach()) * len(indices)
        history.append(total / len(features))
        print(json.dumps({"epoch": epoch + 1, "loss": history[-1], "elapsed_seconds": round(time.monotonic() - started, 1)}), flush=True)
    model.eval()
    exported = torch.nn.Sequential(model, torch.nn.Sigmoid())
    fixtures = features[torch.linspace(0, len(features) - 1, 8).long()]
    path = output / "candidate.onnx"
    torch.onnx.export(exported, fixtures[:1], str(path), opset_version=17, input_names=["logmel"], output_names=["score"],
                      dynamic_axes={"logmel": {0: "batch"}, "score": {0: "batch"}}, dynamo=False)
    onnx.checker.check_model(str(path))
    options = ort.SessionOptions()
    options.intra_op_num_threads, options.inter_op_num_threads = 1, 1
    runtime = ort.InferenceSession(str(path), options, providers=["CPUExecutionProvider"])
    with torch.inference_mode():
        expected = exported(fixtures).numpy()
    parity = float(np.max(np.abs(expected - runtime.run(None, {"logmel": fixtures.numpy()})[0])))
    if parity > 1e-5:
        raise ValueError("ONNX parity failed")
    result = {"schema": "fono-wake-lira-training-result@1", "qualified": False,
              "model_sha256": common.digest(path), "config_sha256": common.digest(output / "config.json"),
              "model_bytes": path.stat().st_size, "parameters": sum(p.numel() for p in model.parameters()),
              "positive_windows": positives, "negative_windows": negatives, "excluded_long_synthetic": excluded,
              "public_clips": len(public), "public_seconds": sum(map(len, public)) / common.RATE,
              "loss_by_epoch": history, "elapsed_seconds": round(time.monotonic() - started, 2),
              "parity_max_abs_error": parity, "owner_voice_used_for_training": False}
    common.write_json(output / "training-result.json", result)
    print(json.dumps(result), flush=True)


def evaluate(args):
    import numpy as np
    import torch
    import onnxruntime as ort
    root = Path(args.model_dir)
    config = json.loads((root / "config.json").read_text(encoding="utf-8"))
    training = json.loads((root / "training-result.json").read_text(encoding="utf-8"))
    if (config["phrase_id"] != PHRASE_ID or config["phrase"] != PHRASE
            or config["source_sha256"] != common.digest(__file__)
            or config["frontend_sha256"] != common.digest(common.__file__)
            or config["recording_loader_sha256"] != common.digest(recordings.__file__)
            or config["threshold"] != common.THRESHOLD
            or common.digest(root / "config.json") != training["config_sha256"]
            or common.digest(root / "candidate.onnx") != training["model_sha256"]):
        raise ValueError("frozen model, source or operating point changed")
    rows = [r for r in recordings.load_recordings(args.recordings_dir) if r["phrase_id"] == PHRASE_ID]
    if not rows or len({r["sample"]["sha256"] for r in rows}) != len(rows):
        raise ValueError("empty or duplicate private dev set")
    output = Path(args.output_dir)
    output.mkdir(parents=True, exist_ok=False)
    torch.set_num_threads(4)
    options = ort.SessionOptions()
    options.intra_op_num_threads, options.inter_op_num_threads = 1, 1
    runtime = ort.InferenceSession(str(root / "candidate.onnx"), options, providers=["CPUExecutionProvider"])
    groups, traces = collections.defaultdict(list), []
    started = time.monotonic()
    for row in rows:
        if time.monotonic() - started > 120:
            raise TimeoutError("bounded private replay exhausted")
        pcm = np.frombuffer(row["pcm"], dtype="<i2").astype(np.float32) / 32768
        ends, windows = common.stream_windows(pcm)
        scores = []
        for i in range(0, len(windows), common.BATCH):
            scores.extend(runtime.run(None, {"logmel": common.frontend(windows[i:i + common.BATCH]).numpy()})[0].ravel().tolist())
        events = common.events(scores, ends)
        s = row["sample"]
        private = {"id": s["id"], "sha256": s["sha256"], "ends_samples": ends, "scores": scores,
                   "events_samples": events, "detected": bool(events), "events": len(events),
                   "seconds": row["seconds"], "digital_silence": row["digital_silence"],
                   "clipped_samples": int(np.sum(np.abs(pcm) >= 32767 / 32768))}
        traces.append(private)
        groups[(s["language"], s["kind"], s["condition"])].append(private)
    report = {"schema": "fono-wake-lira-owner-replay@1", "phrase_id": PHRASE_ID,
              "model_sha256": training["model_sha256"], "config_sha256": training["config_sha256"],
              "threshold": common.THRESHOLD, "mode": "fast-file-causal-replay", "stride_samples": common.STRIDE,
              "left_context_samples": common.WINDOW, "synthetic_tail_samples": common.TAIL,
              "labels": "owner-declared dev; phrase boundaries unannotated; clip hit is not qualified recall",
              "qualified": False, "latency_measured": False, "idle_cpu_measured": False,
              "long_false_activations_measured": False, "owner_voice_used_for_training": False,
              "wall_seconds": round(time.monotonic() - started, 3),
              "groups": [{"language": k[0], "kind": k[1], "condition": k[2], "clips": len(v),
                          "clips_with_detection": sum(r["detected"] for r in v), "events": sum(r["events"] for r in v),
                          "seconds": round(sum(r["seconds"] for r in v), 3),
                          "digital_silence_clips": sum(r["digital_silence"] for r in v),
                          "clipped_samples": sum(r["clipped_samples"] for r in v)} for k, v in sorted(groups.items())]}
    common.write_json(output / "private-traces.json", traces)
    common.write_json(output / "aggregate.json", report)
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    training = sub.add_parser("train")
    training.add_argument("--training-manifest", required=True)
    training.add_argument("--public-speech", required=True)
    training.add_argument("--output-dir", required=True)
    training.add_argument("--max-seconds", type=int, default=180)
    replay = sub.add_parser("evaluate")
    replay.add_argument("--model-dir", required=True)
    replay.add_argument("--recordings-dir", required=True)
    replay.add_argument("--output-dir", required=True)
    args = parser.parse_args()
    train(args) if args.command == "train" else evaluate(args)
