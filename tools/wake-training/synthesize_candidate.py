"""One bounded local synthetic-training corpus. Owner recordings are never inputs to TTS."""
from __future__ import annotations
import argparse
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import random
import time

STYLES = [
    "An adult man, medium pitch, clear natural speech",
    "An adult woman, medium pitch, clear natural speech",
    "An older man, low pitch, calm natural speech",
    "A young adult woman, slightly higher pitch, relaxed natural speech",
]
TARGETS = {"ru": "Эй, Фоно.", "en": "Hey Fono."}
NEGATIVES = {
    "ru": ["Эй, Фома.", "Эй, Вова.", "Привет, Фоно.", "Эй, фото.", "Это фото.",
           "Просто фон.", "Фоно.", "Да, хорошо.", "Открой окно."],
    "en": ["Hey phone.", "Hey photo.", "Hey follow.", "Hi Fono.", "Fono.",
           "A photo.", "It's a phone.", "Very good.", "Open the window."],
}

def digest(path):
    out = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(4 * 1024 * 1024), b""):
            out.update(block)
    return out.hexdigest()

def verify_cache(directory, manifest):
    if manifest.get("repo") != "openbmb/VoxCPM2" or manifest.get("revision") != "32279effe8c19989596f05d353d1447f51d9e915":
        raise ValueError("unexpected TTS source identity")
    verified = {}
    for entry in manifest["files"]:
        name = entry["path"]
        if Path(name).name != name:
            raise ValueError("unexpected asset path")
        path = Path(directory) / name
        if not path.is_file() or path.stat().st_size != entry["size"]:
            raise ValueError("local TTS asset missing or has different size")
        sha256 = digest(path)
        if entry.get("sha256"):
            valid = sha256 == entry["sha256"]
        else:
            content = path.read_bytes()
            valid = hashlib.sha1(f"blob {len(content)}\0".encode() + content).hexdigest() == entry["git_sha1"]
        if not valid:
            raise ValueError("local TTS asset hash mismatch")
        verified[name] = sha256
    return verified

def jobs():
    result = []
    for language, target in TARGETS.items():
        for style_index, style in enumerate(STYLES):
            for repeat in range(3):
                result.append({"language": language, "positive": True, "text": target,
                               "style": style, "seed": 1000 + len(result), "speaker": f"design-{style_index}"})
            for text in NEGATIVES[language]:
                result.append({"language": language, "positive": False, "text": text,
                               "style": style, "seed": 1000 + len(result), "speaker": f"design-{style_index}"})
    return result

def run(args):
    # All framework access stays offline; only already verified local weights can load.
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    os.environ["HF_HUB_DISABLE_TELEMETRY"] = "1"
    identity = json.loads(Path(__file__).with_name("voxcpm2.json").read_text(encoding="utf-8"))
    verified = verify_cache(args.model_dir, identity)
    if importlib.metadata.version("voxcpm") != "2.0.3":
        raise ValueError("expected pinned voxcpm 2.0.3")
    output = Path(args.output_dir)
    output.mkdir(parents=True, exist_ok=False)
    import numpy as np
    import soundfile as sf
    from scipy.signal import resample_poly
    import torch
    from voxcpm import VoxCPM
    torch.set_num_threads(4)
    if not torch.cuda.is_available():
        raise ValueError("bounded candidate requires prepared CUDA runtime")
    free, total = torch.cuda.mem_get_info()
    if free < 10 * 1024**3:
        raise ValueError("insufficient free GPU memory; other models remain untouched")
    torch.cuda.set_per_process_memory_fraction(0.60)
    started = time.monotonic()
    print(json.dumps({"phase": "loading_local_tts", "weights_bytes": sum(e["size"] for e in identity["files"])}), flush=True)
    model = VoxCPM.from_pretrained(str(Path(args.model_dir).resolve()), device="cuda", optimize=False,
                                  load_denoiser=False, local_files_only=True)
    if type(model.tts_model).__name__ != "VoxCPM2Model" or model.tts_model.sample_rate != 48000:
        raise ValueError("unexpected TTS architecture or sample rate")
    entries = []
    for index, job in enumerate(jobs()):
        if time.monotonic() - started > args.max_seconds:
            # Keep the completed prefix explicitly partial; never quietly restart generation.
            break
        random.seed(job["seed"])
        np.random.seed(job["seed"])
        torch.manual_seed(job["seed"])
        with torch.inference_mode():
            audio = np.asarray(model.generate(text=f"({job['style']}){job['text']}", cfg_value=2.0,
                inference_timesteps=10, normalize=False, denoise=False, retry_badcase=False, max_len=256), dtype=np.float32)
        if audio.ndim != 1 or not audio.size or not np.isfinite(audio).all() or not np.any(audio):
            raise ValueError("invalid synthetic audio")
        audio = resample_poly(audio, 1, 3).astype(np.float32)
        # Explicit gain normalization applies only to generated training data.
        audio *= min(1.0, 0.95 / max(0.001, float(np.max(np.abs(audio)))))
        name = f"synthetic-{index:03d}.wav"
        sf.write(output / name, audio, 16000, subtype="PCM_16")
        entries.append({**job, "id": f"synthetic-{index:03d}", "wav": name, "sha256": digest(output/name),
                        "samples": int(audio.size), "origin": "synthetic", "split": "train"})
        if (index+1) % 8 == 0:
            print(json.dumps({"phase":"synthesis", "completed":index+1, "total":len(jobs()),
                              "elapsed_seconds":round(time.monotonic()-started,1)}), flush=True)
    if not entries:
        raise TimeoutError("fixed synthesis budget produced no complete clips")
    complete = len(entries) == len(jobs())
    manifest = {"schema":"fono-wake-synthetic-candidate@1", "entries":entries, "tts_repo":identity["repo"],
                "tts_revision":identity["revision"], "tts_sha256":verified, "owner_voice_used":False,
                "reference_audio_used":False, "source_script_sha256":digest(__file__),
                "synthesis_complete":complete, "planned_clips":len(jobs()),
                "stop_reason":"all_planned_clips" if complete else "fixed_synthesis_time_budget",
                "voxcpm_version":importlib.metadata.version("voxcpm"), "torch_version":torch.__version__,
                "gpu":torch.cuda.get_device_name(), "peak_vram_bytes":torch.cuda.max_memory_reserved(),
                "elapsed_seconds":round(time.monotonic()-started,2)}
    (output/"training-data.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({"phase":"completed" if complete else "partial", "clips":len(entries), "seconds":sum(e["samples"] for e in entries)/16000,
                      "elapsed_seconds":manifest["elapsed_seconds"], "peak_vram_bytes":manifest["peak_vram_bytes"]}),flush=True)

if __name__ == "__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model-dir",required=True)
    parser.add_argument("--output-dir",required=True)
    parser.add_argument("--max-seconds",type=int,default=600)
    run(parser.parse_args())
