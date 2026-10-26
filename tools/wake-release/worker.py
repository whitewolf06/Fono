"""Packaged inference only. No device, network, training or recording files.

The streaming detector and numerical frontend are generated verbatim from the
owner-accepted dev sources. This module only loads pinned assets and speaks the
same bounded protocol as the dev worker.
"""
from __future__ import annotations
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import struct
import sys
from types import SimpleNamespace


def checked_file(root, relative, expected):
    path = root / relative
    if path.is_symlink() or path.is_junction() or not path.is_file():
        raise ValueError("regular asset required")
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(4 * 1024**2), b""):
            digest.update(block)
    if digest.hexdigest() != expected:
        raise ValueError("pinned asset changed")
    return path


def prepare(root, reply):
    reply({"loading": "runtime"})
    import numpy as np
    import onnxruntime as ort
    ort.disable_telemetry_events()
    manifest = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
    if manifest["schema"] != "fono-lira-runtime@1" or manifest["protocol"] != 2:
        raise ValueError("invalid runtime manifest")
    for name, version in manifest["versions"].items():
        actual = ".".join(map(str, sys.version_info[:3])) if name == "python" else importlib.metadata.version(name)
        if actual != version:
            raise ValueError("runtime version mismatch")
    reply({"loading": "models"})
    model = checked_file(root, "models/candidate.onnx", manifest["cnn"])
    tone_model = checked_file(root, "models/tone.onnx", manifest["tone"])
    tokens = checked_file(root, "models/tokens.txt", manifest["tokens"])
    options = ort.SessionOptions()
    options.intra_op_num_threads, options.inter_op_num_threads = 1, 1
    cnn = ort.InferenceSession(str(model), options, providers=["CPUExecutionProvider"])
    options = ort.SessionOptions()
    options.intra_op_num_threads, options.inter_op_num_threads = 2, 1
    options.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
    session = ort.InferenceSession(str(tone_model), options, providers=["CPUExecutionProvider"])
    metadata = session.get_modelmeta().custom_metadata_map
    if (metadata.get("model_type") != "t-one" or metadata.get("sample_rate") != "8000"
            or metadata.get("state_dim") != "219729" or metadata.get("frame_length_ms") != "300"
            or [x.name for x in session.get_inputs()] != ["signal", "state"]
            or [x.name for x in session.get_outputs()] != ["logprobs", "state_next"]):
        raise ValueError("unexpected T-one tensor contract")
    vocabulary = {}
    for line in tokens.read_text(encoding="utf-8").splitlines():
        fields = line.strip().split()
        token, identifier = (" ", int(fields[0])) if len(fields) == 1 else (fields[0], int(fields[1]))
        vocabulary[token] = identifier
    return np, cnn, SimpleNamespace(np=np, session=session, vocabulary=vocabulary, blank=vocabulary["<blk>"]), manifest


def run():
    # Fixed before loading NumPy/ORT. No inherited user package configuration.
    os.environ["OPENBLAS_NUM_THREADS"] = "1"
    os.environ["OMP_NUM_THREADS"] = "1"
    from live_worker import Stream, classify_contexts, exact_read, reply
    import candidate as common
    import live_runtime
    root = Path(__file__).resolve().parent.parent
    np, cnn, tone, manifest = prepare(root, reply)
    reply({"loading": "warmup"})
    cnn.run(None, {"logmel": live_runtime.frontend(np.zeros((1, common.WINDOW), dtype=np.float32))})
    live_runtime.verify(tone, np.zeros(common.RATE, dtype=np.float32))
    stream = Stream(
        lambda window: classify_contexts(window, lambda batch:
            cnn.run(None, {"logmel": live_runtime.frontend(batch)})[0]),
        lambda snapshot: live_runtime.verify(tone, snapshot),
    )
    reply({"protocol": 2, "cnn": manifest["cnn"], "verifier": manifest["verifier"]})
    while True:
        kind, count = struct.unpack("<BI", exact_read(sys.stdin.buffer, 5))
        if kind == 0 and count == 0:
            stream.reset()
            reply({"processed": 0, "detected": False})
        elif kind == 1 and count == common.STRIDE:
            speech = exact_read(sys.stdin.buffer, 1)[0]
            if speech not in (0, 1):
                raise ValueError("invalid VAD flag")
            values = np.frombuffer(exact_read(sys.stdin.buffer, count * 2), dtype="<i2").astype(np.float32) / 32768
            reply(stream.accept(values, bool(speech)))
        else:
            raise ValueError("invalid bounded request")


if __name__ == "__main__":
    try:
        run()
    except (EOFError, BrokenPipeError):
        pass
    except Exception:
        # Protocol diagnostics never disclose input, transcripts or local paths.
        print('{"error":"worker_failed"}', flush=True)
        sys.exit(1)
