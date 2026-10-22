"""Single explicit desktop-dev WAV check. No microphone, files, training or transcripts."""
from __future__ import annotations
import importlib.metadata
import io
import json
from pathlib import Path
import sys
import time
import wave

import candidate as common
import lira
import lira_ctc

ROOT = Path(__file__).resolve().parents[2]
POINT_SHA256 = "d3ea4079b329755bcdd42618cec915aa4952554479eb4b0999689558305ee47e"
CNN_SHA256 = "a9c4e2595669605325b97b7aeec62781c58073e9966a3850d360328bddaa541c"
MAX_WAV_BYTES = 22 * common.RATE * 2 + 4096


def read_input(stream):
    raw = stream.read(MAX_WAV_BYTES + 1)
    if not 44 < len(raw) <= MAX_WAV_BYTES:
        raise ValueError("invalid bounded WAV")
    with wave.open(io.BytesIO(raw), "rb") as audio:
        if (audio.getnchannels(), audio.getsampwidth(), audio.getframerate(), audio.getcomptype()) != (1, 2, common.RATE, "NONE"):
            raise ValueError("expected PCM16 mono 16 kHz")
        count = audio.getnframes()
        if not common.RATE // 4 <= count <= 22 * common.RATE:
            raise ValueError("invalid duration")
        pcm = audio.readframes(count)
        if len(pcm) != count * 2:
            raise ValueError("truncated PCM")
        return pcm


def check(pcm_bytes):
    started = time.monotonic()
    import numpy as np
    import torch
    import onnxruntime as ort

    model = ROOT / "build/wake-lira-candidate/model-01"
    config = json.loads((model / "config.json").read_text(encoding="utf-8"))
    point_path = ROOT / "tools/wake-training/lira-ctc-exact/config.json"
    if (common.digest(point_path) != POINT_SHA256
            or common.digest(model / "candidate.onnx") != CNN_SHA256
            or config["source_sha256"] != common.digest(lira.__file__)
            or config["frontend_sha256"] != common.digest(common.__file__)
            or config["threshold"] != common.THRESHOLD
            or config["window_samples"] != common.WINDOW
            or config["stride_samples"] != common.STRIDE
            or config["tail_samples"] != common.TAIL
            or config["cooldown_samples"] != common.COOLDOWN):
        raise ValueError("frozen CNN/operating point changed")
    point = json.loads(point_path.read_text(encoding="utf-8"))
    if (point["source_sha256"] != common.digest(lira_ctc.__file__)
            or point["decision_rule"] != "greedy_whole_phrase"
            or point["cutoff"] is not None
            or point["context_samples"] != lira_ctc.CONTEXT
            or point["lookahead_samples"] != lira_ctc.LOOKAHEAD):
        raise ValueError("frozen verifier changed")
    for name in ("numpy", "scipy", "torch", "onnxruntime"):
        if importlib.metadata.version(name) != config["versions"][name]:
            raise ValueError("runtime version mismatch")
    torch.set_num_threads(4)
    options = ort.SessionOptions()
    options.intra_op_num_threads, options.inter_op_num_threads = 1, 1
    cnn = ort.InferenceSession(str(model / "candidate.onnx"), options, providers=["CPUExecutionProvider"])
    tone = lira_ctc.Tone(ROOT / "build/wake-models" / lira_ctc.DIRECTORY, ROOT / "build/wake-models/ru.tar.bz2")
    if tone.identity != point["model_identity"]:
        raise ValueError("verifier identity changed")
    pcm = np.frombuffer(pcm_bytes, dtype="<i2").astype(np.float32) / 32768
    ends, windows = common.stream_windows(pcm)
    scores = []
    for i in range(0, len(windows), common.BATCH):
        scores.extend(cnn.run(None, {"logmel": common.frontend(windows[i:i + common.BATCH]).numpy()})[0].ravel().tolist())
    proposals = common.events(scores, ends)
    verified = 0
    for event in proposals:
        window = pcm[max(0, event - lira_ctc.CONTEXT):min(len(pcm), event + lira_ctc.LOOKAHEAD)]
        verified += int(tone.score(window)["greedy_whole_phrase"])
    return {"detected": verified > 0, "proposals": len(proposals), "events": verified,
            "processingMs": round((time.monotonic() - started) * 1000),
            "durationMs": len(pcm_bytes) * 1000 // (2 * common.RATE)}


if __name__ == "__main__":
    try:
        print(json.dumps(check(read_input(sys.stdin.buffer))))
    except Exception:
        # Do not leak voice, transcripts, file paths or dependency exceptions to IPC.
        print("wake_sample_check_failed", file=sys.stderr)
        sys.exit(1)
