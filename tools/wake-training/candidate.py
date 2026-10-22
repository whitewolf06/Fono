"""Bounded experimental log-mel CNN: synthetic train, private recording dev, no app IPC.

This is a research candidate, never a qualified or automatically installed model.
The operating point is fixed before evaluation; owner recordings cannot train it.
"""
from __future__ import annotations

import argparse
import collections
import hashlib
import json
from pathlib import Path
import random
import time
import wave

SEED = 20261022
RATE = 16000
WINDOW = 64000
STRIDE = 1280
TAIL = 10240
THRESHOLD = 0.8
COOLDOWN = 32000
EPOCHS = 16
BATCH = 64


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write_json(path, value):
    Path(path).write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf-8")


def read_pcm(path):
    """Strict bounded PCM input; never substitute a missing/invalid clip with silence."""
    import numpy as np
    if not 44 < Path(path).stat().st_size <= RATE * 22 * 2 + 4096:
        raise ValueError("WAV exceeds bounded sample size or is empty")
    with wave.open(str(path), "rb") as audio:
        if (audio.getnchannels(), audio.getsampwidth(), audio.getframerate(), audio.getcomptype()) != (1, 2, RATE, "NONE"):
            raise ValueError("expected PCM16 mono 16 kHz")
        count = audio.getnframes()
        pcm = audio.readframes(count)
        if count == 0 or len(pcm) != count * 2:
            raise ValueError("empty or truncated WAV")
    return np.frombuffer(pcm, dtype="<i2").astype(np.float32) / 32768.0


def training_entries(path):
    """Only explicit synthetic/train provenance. No dev/test/owner fallback."""
    manifest = json.loads(Path(path).read_text(encoding="utf-8"))
    if (manifest.get("schema") != "fono-wake-synthetic-candidate@1"
            or manifest.get("owner_voice_used") is not False
            or manifest.get("reference_audio_used") is not False):
        raise ValueError("synthetic-only provenance required")
    entries = manifest.get("entries", [])
    if not entries or len(entries) > 200:
        raise ValueError("invalid bounded synthetic corpus")
    seen = set()
    for entry in entries:
        name = entry["wav"]
        if (entry.get("origin") != "synthetic" or entry.get("split") != "train"
                or entry.get("language") not in ("ru", "en")
                or type(entry.get("positive")) is not bool
                or Path(name).name != name or "/" in name or "\\" in name
                or name in seen):
            raise ValueError("invalid training entry or split contamination")
        seen.add(name)
        audio = Path(path).parent / name
        if digest(audio) != entry["sha256"]:
            raise ValueError("synthetic training hash mismatch")
        samples = read_pcm(audio)
        if len(samples) != entry["samples"]:
            raise ValueError("synthetic sample count mismatch")
    if {(e["language"], e["positive"]) for e in entries} != {
            (language, label) for language in ("ru", "en") for label in (False, True)}:
        raise ValueError("both languages and labels are required")
    return manifest


def owner_entries(directory):
    """Local recording bridge: labels are user declarations, never inferred timestamps."""
    records = []
    for folder in sorted(Path(directory).iterdir()):
        if not folder.is_dir() or folder.name.startswith("."):
            continue
        if folder.is_symlink() or folder.is_junction():
            raise ValueError("linked owner recording directories are unsupported")
        metadata = json.loads((folder / "recording.json").read_text(encoding="utf-8"))
        if (metadata.get("schema") != "fono-wake-recording@1"
                or metadata.get("permission") != "consented-local-only"
                or metadata.get("split") != "dev"):
            raise ValueError("explicit local-only dev consent required")
        sample = metadata["sample"]
        if (sample["id"] != folder.name or sample["language"] not in ("ru", "en")
                or sample["kind"] not in ("phrase", "other_speech", "background")
                or sample["condition"] not in ("quiet", "noisy")):
            raise ValueError("invalid declared owner group")
        audio = folder / "recording.wav"
        if digest(audio) != sample["sha256"]:
            raise ValueError("owner WAV changed")
        pcm = read_pcm(audio)
        if abs(int(len(pcm) / 16) - sample["durationMs"]) > 1:
            raise ValueError("owner duration mismatch")
        records.append({"sample": sample, "pcm": pcm, "wav": audio,
                        "annotated": metadata.get("annotation") is not None})
    if not 1 <= len(records) <= 200:
        raise ValueError("invalid bounded owner recording count")
    if len({r["sample"]["sha256"] for r in records}) != len(records):
        raise ValueError("duplicate owner audio")
    return records


def stream_windows(pcm):
    """Causal 80 ms replay; 4 s left context and 640 ms synthetic tail are explicit."""
    import numpy as np
    timeline = np.pad(pcm, (WINDOW, TAIL))
    size = len(pcm) + TAIL
    ends = list(range(STRIDE, size + 1, STRIDE))
    if not ends or ends[-1] != size:
        ends.append(size)
    return ends, np.stack([timeline[end:end + WINDOW] for end in ends])


def events(scores, ends):
    """A crossing triggers once; a continuously high score never retriggers by timer."""
    if len(scores) != len(ends):
        raise ValueError("unaligned scores and stream clocks")
    result, until, armed = [], 0, True
    for score, end in zip(scores, ends):
        if not 0 <= float(score) <= 1:
            raise ValueError("invalid classifier score")
        if score < THRESHOLD:
            armed = True
        elif armed and end >= until:
            result.append(end)
            until, armed = end + COOLDOWN, False
    return result


def mel_matrix():
    import numpy as np
    hz_to_mel = lambda hz: 2595 * np.log10(1 + hz / 700)
    edges = 700 * (10 ** (np.linspace(hz_to_mel(20), hz_to_mel(7600), 42) / 2595) - 1)
    bins = np.arange(257) * RATE / 512
    down = (bins[None, :] - edges[:-2, None]) / (edges[1:-1, None] - edges[:-2, None])
    up = (edges[2:, None] - bins[None, :]) / (edges[2:, None] - edges[1:-1, None])
    return np.maximum(0, np.minimum(down, up)).astype(np.float32)


def frontend(pcm, device="cpu"):
    import torch
    values = torch.as_tensor(pcm, dtype=torch.float32, device=device)
    if values.ndim != 2 or values.shape[1] != WINDOW:
        raise ValueError("frontend expects N x 64000 samples")
    spectrum = torch.stft(torch.nn.functional.pad(values, (352, 0)), n_fft=512,
                          hop_length=160, win_length=400,
                          window=torch.hann_window(400, device=device),
                          center=False, return_complex=True).abs().square()
    mel = torch.as_tensor(mel_matrix(), device=device)
    features = (mel @ spectrum).clamp_min(1e-8).log()
    mean = features.mean(dim=(1, 2), keepdim=True)
    std = features.std(dim=(1, 2), keepdim=True, correction=0).clamp_min(1)
    return ((features - mean) / std).unsqueeze(1)


def network():
    import torch.nn as nn
    return nn.Sequential(nn.Conv2d(1, 8, 3, padding=1), nn.ReLU(), nn.AvgPool2d(2),
                         nn.Conv2d(8, 16, 3, padding=1), nn.ReLU(), nn.AvgPool2d(2),
                         nn.Conv2d(16, 24, 3, padding=1), nn.ReLU(), nn.AvgPool2d(2),
                         nn.AdaptiveAvgPool2d((1, 10)), nn.Flatten(),
                         nn.Linear(240, 32), nn.ReLU(), nn.Linear(32, 1))


def augmented(pcm, rng):
    import numpy as np
    # Speed changes are training augmentation, never applied to owner dev audio.
    speed = rng.uniform(0.82, 1.18)
    stretched = np.interp(np.arange(0, len(pcm), speed), np.arange(len(pcm)), pcm).astype(np.float32)
    if len(stretched) > WINDOW:
        return None  # Never silently truncate a target phrase.
    window = np.zeros(WINDOW, dtype=np.float32)
    position = int(rng.integers(0, WINDOW - len(stretched) + 1))
    window[position:position + len(stretched)] = stretched * rng.uniform(0.4, 1.5)
    rms = float(np.sqrt(np.mean(stretched * stretched)))
    window += rng.normal(0, max(0.0001, rms) / 10 ** (rng.uniform(8, 25) / 20), WINDOW).astype(np.float32)
    return np.clip(window, -1, 1)


def train(args):
    import importlib.metadata
    import numpy as np
    import onnx
    import onnxruntime as ort
    import torch
    manifest = training_entries(args.training_manifest)
    output = Path(args.output_dir)
    output.mkdir(parents=True, exist_ok=False)
    torch.set_num_threads(4)
    random.seed(SEED)
    np.random.seed(SEED)
    torch.manual_seed(SEED)
    torch.use_deterministic_algorithms(True)
    rng = np.random.default_rng(SEED)
    # CPU avoids touching an application's GPU model or simultaneous TTS allocation.
    device = "cpu"
    config = {"schema": "fono-logmel-candidate@1", "seed": SEED, "sample_rate": RATE,
              "window_samples": WINDOW, "stride_samples": STRIDE, "tail_samples": TAIL,
              "threshold": THRESHOLD, "cooldown_samples": COOLDOWN, "epochs": EPOCHS,
              "batch_size": BATCH, "optimizer": "AdamW", "learning_rate": 0.001,
              "weight_decay": 0.01, "training_seconds_limit": args.max_seconds,
              "frontend": {"n_fft": 512, "hann": 400, "hop": 160, "left_pad": 352,
                           "mel_bands": 40, "hz": [20, 7600], "log_floor": 1e-8,
                           "normalization": "per-window mean/std, minimum std=1"},
              "architecture": "conv8-16-24,pool2x3,adaptive1x10,fc32,fc1,sigmoid",
              "augmentation": {"speed": [0.82, 1.18], "gain": [0.4, 1.5], "snr_db": [8, 25]},
              "source_script_sha256": digest(__file__),
              "training_manifest_sha256": digest(args.training_manifest),
              "versions": {name: importlib.metadata.version(name) for name in
                           ("numpy", "scipy", "torch", "onnx", "onnxruntime")},
              "owner_voice_used_for_training": False, "qualified": False}
    write_json(output / "config.json", config)  # Fixed before any dev evaluation.
    started = time.monotonic()
    values, labels, excluded, used = [], [], [], []
    for entry in manifest["entries"]:
        pcm = read_pcm(Path(args.training_manifest).parent / entry["wav"])
        if len(pcm) > WINDOW:
            excluded.append({"id": entry["id"], "reason": "longer_than_window"})
            continue
        used.append(entry)
        for _ in range(32 if entry["positive"] else 10):
            augmented_pcm = augmented(pcm, rng)
            if augmented_pcm is None:
                continue
            values.append(augmented_pcm)
            labels.append(float(entry["positive"]))
    if {(e["language"], e["positive"]) for e in used} != {
            (language, label) for language in ("ru", "en") for label in (False, True)}:
        raise ValueError("duration exclusions removed a required language/class")
    # Silence/noise examples are training material, not recorded negatives or qualification.
    for _ in range(48):
        values.append(rng.normal(0, rng.uniform(0, 0.01), WINDOW).astype(np.float32))
        labels.append(0.0)
    features = torch.cat([frontend(np.stack(values[offset:offset + BATCH]), device)
                          for offset in range(0, len(values), BATCH)])
    targets = torch.tensor(labels, dtype=torch.float32).reshape(-1, 1)
    del values
    model = network().to(device)
    optimizer = torch.optim.AdamW(model.parameters(), lr=0.001, weight_decay=0.01)
    positive = int(targets.sum())
    negative = len(targets) - positive
    loss_fn = torch.nn.BCEWithLogitsLoss(pos_weight=torch.tensor([negative / positive]))
    history = []
    for epoch in range(EPOCHS):
        model.train()
        order, total = torch.randperm(len(features)), 0.0
        for indices in order.split(BATCH):
            if time.monotonic() - started > args.max_seconds:
                raise TimeoutError("fixed training budget exhausted; no completed artifact")
            optimizer.zero_grad(set_to_none=True)
            loss = loss_fn(model(features[indices]), targets[indices])
            if not torch.isfinite(loss):
                raise ValueError("nonfinite training loss")
            loss.backward()
            optimizer.step()
            total += float(loss.detach()) * len(indices)
        history.append(total / len(features))
        print(json.dumps({"epoch": epoch + 1, "loss": history[-1],
                          "elapsed_seconds": round(time.monotonic() - started, 1)}), flush=True)
    model.eval()
    exported = torch.nn.Sequential(model, torch.nn.Sigmoid())
    # Adaptive pooling is exported at a fixed feature shape; only batch is dynamic.
    fixtures = features[torch.linspace(0, len(features) - 1, 8).long()]
    model_path = output / "candidate.onnx"
    torch.onnx.export(exported, fixtures[:1], str(model_path), opset_version=17,
                      input_names=["logmel"], output_names=["score"],
                      dynamic_axes={"logmel": {0: "batch"}, "score": {0: "batch"}}, dynamo=False)
    onnx.checker.check_model(str(model_path))
    options = ort.SessionOptions()
    options.intra_op_num_threads, options.inter_op_num_threads = 1, 1
    runtime = ort.InferenceSession(str(model_path), options, providers=["CPUExecutionProvider"])
    with torch.inference_mode():
        expected = exported(fixtures).numpy()
    observed = runtime.run(None, {"logmel": fixtures.numpy()})[0]
    error = float(np.max(np.abs(expected - observed)))
    if error > 1e-5:
        raise ValueError("PyTorch/ONNX parity failed")
    report = {"schema": "fono-logmel-training-result@1", "config_sha256": digest(output / "config.json"),
              "model_sha256": digest(model_path), "model_bytes": model_path.stat().st_size,
              "parameters": sum(p.numel() for p in model.parameters()), "device": device,
              "elapsed_seconds": round(time.monotonic() - started, 2), "loss_by_epoch": history,
              "source_clips_used": len(used), "source_clips_excluded": excluded,
              "augmented_positive_windows": positive, "augmented_negative_windows": negative,
              "onnx_opset": 17, "parity_max_abs_error": error, "qualified": False}
    write_json(output / "training-result.json", report)
    print(json.dumps(report), flush=True)


def evaluate(args):
    import numpy as np
    import onnxruntime as ort
    import torch
    model_dir = Path(args.model_dir)
    config = json.loads((model_dir / "config.json").read_text(encoding="utf-8"))
    training = json.loads((model_dir / "training-result.json").read_text(encoding="utf-8"))
    if (config["source_script_sha256"] != digest(__file__)
            or digest(model_dir / "config.json") != training["config_sha256"]
            or digest(model_dir / "candidate.onnx") != training["model_sha256"]
            or config["threshold"] != THRESHOLD):
        raise ValueError("candidate identity/operating point changed")
    output = Path(args.output_dir)
    output.mkdir(parents=True, exist_ok=False)
    torch.set_num_threads(4)
    options = ort.SessionOptions()
    options.intra_op_num_threads, options.inter_op_num_threads = 1, 1
    runtime = ort.InferenceSession(str(model_dir / "candidate.onnx"), options,
                                   providers=["CPUExecutionProvider"])
    records = owner_entries(args.recordings_dir)
    started = time.monotonic()
    private, groups = [], collections.defaultdict(list)
    for record in records:
        ends, windows = stream_windows(record["pcm"])
        scores = []
        for offset in range(0, len(windows), BATCH):
            features = frontend(windows[offset:offset + BATCH]).numpy()
            scores.extend(runtime.run(None, {"logmel": features})[0].ravel().tolist())
        detections = events(scores, ends)
        if digest(record["wav"]) != record["sample"]["sha256"]:
            raise ValueError("owner WAV changed during replay")
        sample = record["sample"]
        row = {"id": sample["id"], "sha256": sample["sha256"], "events_samples": detections,
               "ends_samples": ends, "scores": scores, "seconds": len(record["pcm"]) / RATE,
               "detected": bool(detections), "event_count": len(detections),
               "all_zero": not bool(np.any(record["pcm"]))}
        private.append(row)
        groups[(sample["language"], sample["kind"], sample["condition"])].append(row)
    report = {"schema": "fono-logmel-owner-replay@1", "model_sha256": training["model_sha256"],
              "config_sha256": training["config_sha256"], "operating_threshold": THRESHOLD,
              "left_context_samples": WINDOW, "synthetic_tail_samples": TAIL,
              "stride_samples": STRIDE, "cooldown_samples": COOLDOWN,
              "mode": "fast-file-replay", "labels": "owner-declared, phrase times unverified",
              "latency_measured": False, "idle_cpu_measured": False, "qualified": False,
              "seconds": round(sum(row["seconds"] for row in private), 3),
              "wall_seconds": round(time.monotonic() - started, 3),
              "groups": [{"language": key[0], "kind": key[1], "condition": key[2],
                          "clips": len(rows), "clips_with_detection": sum(r["detected"] for r in rows),
                          "events": sum(r["event_count"] for r in rows),
                          "seconds": round(sum(r["seconds"] for r in rows), 3),
                          "digital_silence_clips": sum(r["all_zero"] for r in rows)}
                         for key, rows in sorted(groups.items())]}
    # Private file contains audio identities and traces. Only aggregate.json is shareable.
    write_json(output / "private-traces.json", private)
    write_json(output / "aggregate.json", report)
    print(json.dumps(report, indent=2), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    training_parser = sub.add_parser("train")
    training_parser.add_argument("--training-manifest", required=True)
    training_parser.add_argument("--output-dir", required=True)
    training_parser.add_argument("--max-seconds", type=int, default=180)
    evaluation_parser = sub.add_parser("evaluate")
    evaluation_parser.add_argument("--model-dir", required=True)
    evaluation_parser.add_argument("--recordings-dir", required=True)
    evaluation_parser.add_argument("--output-dir", required=True)
    arguments = parser.parse_args()
    (train if arguments.command == "train" else evaluate)(arguments)
