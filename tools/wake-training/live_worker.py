"""Warm dev-only Lira cascade. Ordered PCM from AudioHub; no device, disk audio or network."""
from __future__ import annotations
import json
import os
from pathlib import Path
import struct
import sys
import time

import candidate as common
import dev_check
import lira_ctc
import live_runtime

# An early CNN crossing may precede the completed name. Keep exact confirmation
# mandatory, but allow two bounded continuations of the same rejected proposal.
MAX_REJECTED_RETRIES = 2
FOCUSED_SAMPLES = common.WINDOW // 2
PREFILL_CHUNKS_PER_FRAME = 2


def validate_policy():
    policy = json.loads(Path(__file__).with_name("lira-live-policy.json").read_text(encoding="utf-8"))
    expected = {"schema": "fono-lira-live-policy@3", "cnn_sha256": dev_check.CNN_SHA256,
                "verifier_config_sha256": dev_check.POINT_SHA256, "sample_rate": common.RATE,
                "context_samples": [common.WINDOW, FOCUSED_SAMPLES], "threshold": common.THRESHOLD,
                "stride_samples": common.STRIDE, "lookahead_samples": common.TAIL,
                "rejected_proposal_retries": MAX_REJECTED_RETRIES, "retry_horizon_samples": common.WINDOW,
                "verifier_prefill_chunks_per_frame": PREFILL_CHUNKS_PER_FRAME,
                "verifier_schedule": "immutable FIR-safe prefix during unchanged lookahead; exact final decision",
                "acceptance_rule": "unchanged exact greedy whole phrase from T-one",
                "owner_voice_used_for_training": False, "qualified": False,
                "runtime_source_sha256": common.digest(Path(__file__).with_name("live_runtime.py")),
                "runtime_numerics": "numpy-stft512-hann400-hop160; kaiser5-half-rate; original exact greedy phrase"}
    if any(policy.get(key) != value for key, value in expected.items()):
        raise ValueError("live policy drift")


def classify_contexts(window, predict):
    """The same CNN checks full context and a zero-left-padded recent half.

    Training clips contain a single utterance. Earlier ordinary conversation
    must not occupy every candidate window until four seconds have elapsed.
    Exact CTC still decides whether any proposal is a real wake phrase.
    """
    import numpy as np
    focused = window.copy()
    focused[:-FOCUSED_SAMPLES] = 0
    scores = np.asarray(predict(np.stack((window, focused)))).reshape(-1)
    if len(scores) != 2 or not np.isfinite(scores).all() or np.any((scores < 0) | (scores > 1)):
        raise ValueError("invalid context classifier result")
    return float(scores.max())


class Stream:
    def __init__(self, classify, verify, prefill=None):
        import numpy as np
        self.np, self.classify, self.verify, self.prefill = np, classify, verify, prefill
        # Technical counters only. Never retain microphone PCM or verifier text.
        self.stats = {"frames": 0, "speech_frames": 0, "max_abs": 0.0, "max_cnn_score": 0.0,
                      "proposals": 0, "checks": 0, "accepted": 0, "resets": -1,
                      "retries": 0, "max_classify_ms": 0.0, "max_verify_ms": 0.0,
                      "max_verify_step_ms": 0.0}
        self.last_check = None
        self.reset()

    def reset(self):
        self.stats["resets"] += 1
        self.pcm = self.np.zeros(0, dtype=self.np.float32)
        self.processed, self.until = 0, 0
        self.armed, self.pending = True, None
        self.retry, self.attempts, self.retry_deadline = False, 0, 0
        self.pending_verifier, self.pending_verify_ms = None, 0.0

    def accept(self, values, speech):
        if len(values) != common.STRIDE or not self.np.isfinite(values).all():
            raise ValueError("one ordered 80ms frame required")
        self.pcm = self.np.concatenate((self.pcm, values))[-(common.WINDOW + common.TAIL):]
        self.processed += len(values)
        self.stats["frames"] += 1
        self.stats["max_abs"] = max(self.stats["max_abs"], float(self.np.abs(values).max()))
        score = 0.0
        if speech:
            self.stats["speech_frames"] += 1
            window = self.pcm[-common.WINDOW:]
            if len(window) < common.WINDOW:
                window = self.np.pad(window, (common.WINDOW - len(window), 0))
            started = time.perf_counter()
            score = float(self.classify(window))
            self.stats["max_classify_ms"] = max(self.stats["max_classify_ms"],
                                                (time.perf_counter() - started) * 1000)
        if not 0 <= score <= 1:
            raise ValueError("invalid classifier result")
        self.stats["max_cnn_score"] = max(self.stats["max_cnn_score"], score)
        if self.retry and self.processed > self.retry_deadline:
            self.retry = False
        if score < common.THRESHOLD:
            self.armed = True
        elif self.armed and self.pending is None and (self.retry or self.processed >= self.until):
            if self.retry:
                self.stats["retries"] += 1
            else:
                self.attempts = 0
                self.retry_deadline = self.processed + common.WINDOW
            self.attempts += 1
            self.retry = False
            self.stats["proposals"] += 1
            self.pending = self.processed
            self.pending_verifier = self.prefill() if self.prefill is not None else None
            self.pending_verify_ms = 0.0
            self.until, self.armed = self.processed + common.COOLDOWN, False
        detected = False
        proposal = self.pending
        if proposal is not None and (self.pending_verifier is not None
                                     or self.processed >= proposal + common.TAIL):
            start = max(0, proposal - common.WINDOW)
            available_start = self.processed - len(self.pcm)
            if start < available_start:
                raise ValueError("verifier audio no longer available")
            end = min(self.processed, proposal + common.TAIL)
            snapshot = self.pcm[start - available_start:end - available_start]
            final = self.processed >= proposal + common.TAIL
            started = time.perf_counter()
            if self.pending_verifier is not None:
                decision = self.pending_verifier.advance(snapshot, final=final,
                                                         max_chunks=PREFILL_CHUNKS_PER_FRAME)
                if not final and decision is not None:
                    raise ValueError("prefill may not announce readiness")
                detected = bool(decision) if final else False
            else:
                detected = bool(self.verify(snapshot))
            step_ms = (time.perf_counter() - started) * 1000
            self.pending_verify_ms += step_ms
            self.stats["max_verify_step_ms"] = max(self.stats["max_verify_step_ms"], step_ms)
            if not final:
                return {"processed": self.processed, "detected": False}
            self.stats["max_verify_ms"] = max(self.stats["max_verify_ms"], self.pending_verify_ms)
            self.stats["checks"] += 1
            self.stats["accepted"] += int(detected)
            self.pending = None
            self.pending_verifier = None
            self.last_check = {"proposal_sample": proposal, "available_sample": self.processed,
                               "verify_ms": self.pending_verify_ms, "finalize_ms": step_ms,
                               "accepted": detected, "attempt": self.attempts}
            self.retry = (not detected and self.attempts <= MAX_REJECTED_RETRIES
                          and self.processed < self.retry_deadline)
            if self.retry:
                self.armed = True
        return {"processed": self.processed, "detected": detected}


class Diagnostics:
    """Bounded debug counter snapshot; no audio, text, device names or paths inside."""
    def __init__(self, path):
        self.path, self.next_write = Path(path), 0.0

    def write(self, stream, force=False):
        now = time.monotonic()
        if not force and now < self.next_write:
            return
        self.next_write = now + 1.0
        value = {"schema": "fono-lira-live-counters@1", "pid": os.getpid(),
                 "updated_at": time.time(), "processed": stream.processed,
                 "last_check": stream.last_check, **stream.stats}
        temporary = self.path.with_suffix(".tmp")
        try:
            temporary.write_text(json.dumps(value), encoding="utf-8")
            temporary.replace(self.path)
        except OSError:
            # Failure of optional diagnostics must not interrupt detection.
            pass


def exact_read(source, count):
    result = bytearray()
    while len(result) < count:
        block = source.read(count - len(result))
        if not block:
            raise EOFError
        result.extend(block)
    return bytes(result)


def reply(value):
    print(json.dumps(value), flush=True)


def run():
    validate_policy()
    # Small real-time matrix products must not start a large BLAS thread pool.
    os.environ["OPENBLAS_NUM_THREADS"] = "1"
    os.environ["OMP_NUM_THREADS"] = "1"
    np, cnn, tone = dev_check.prepare(lambda stage: reply({"loading": stage}), inference_only=True)
    # First-use native/library work belongs to Loading, before opening the mic.
    # Synthetic silence warms sessions only; it is never quality exposure.
    reply({"loading": "warmup"})
    cnn.run(None, {"logmel": live_runtime.frontend(np.zeros((1, common.WINDOW), dtype=np.float32))})
    live_runtime.verify(tone, np.zeros(common.RATE, dtype=np.float32))
    stream = Stream(
        lambda window: classify_contexts(window, lambda batch:
            cnn.run(None, {"logmel": live_runtime.frontend(batch)})[0]),
        lambda snapshot: live_runtime.verify(tone, snapshot),
        prefill=lambda: live_runtime.IncrementalVerifier(tone),
    )
    diagnostics = Diagnostics(dev_check.ROOT / "build/checks/lira-live-counters.json")
    diagnostics.write(stream, force=True)
    reply({"protocol": 2, "cnn": dev_check.CNN_SHA256, "verifier": dev_check.POINT_SHA256})
    while True:
        kind, count = struct.unpack("<BI", exact_read(sys.stdin.buffer, 5))
        if kind == 0 and count == 0:
            stream.reset()
            diagnostics.write(stream, force=True)
            reply({"processed": 0, "detected": False})
        elif kind == 1 and count == common.STRIDE:
            speech = exact_read(sys.stdin.buffer, 1)[0]
            if speech not in (0, 1):
                raise ValueError("invalid VAD flag")
            values = np.frombuffer(exact_read(sys.stdin.buffer, count * 2), dtype="<i2").astype(np.float32) / 32768
            result = stream.accept(values, bool(speech))
            diagnostics.write(stream, force=result["detected"])
            reply(result)
        else:
            raise ValueError("invalid bounded request")


def run_probed():
    # Explicit local diagnosis only: code stacks, never PCM, texts or locals.
    if os.environ.get("FONO_WAKE_DEV_PROBE") != "1":
        return run()
    import faulthandler
    import threading
    directory = dev_check.ROOT / "build/checks/lira-runtime-probes"
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / f"worker-{os.getpid()}.log").open("w", encoding="utf-8") as output:
        output.write(json.dumps({"pid": os.getpid(), "started_at": time.time(),
            "kind": "periodic_code_stack_sample_not_a_failure", "interval_seconds": 2,
            "maximum_seconds": 180}) + "\n")
        output.flush()
        faulthandler.dump_traceback_later(2, repeat=True, file=output)
        stop = threading.Timer(180, faulthandler.cancel_dump_traceback_later)
        stop.daemon = True
        stop.start()
        try:
            return run()
        finally:
            stop.cancel()
            faulthandler.cancel_dump_traceback_later()


if __name__ == "__main__":
    try:
        run_probed()
    except (EOFError, BrokenPipeError):
        pass
    except Exception:
        # Only technical failure; never output transcripts, private PCM or paths.
        reply({"error": "worker_failed"})
        sys.exit(1)
