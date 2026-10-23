"""Warm dev-only Lira cascade. Ordered PCM from AudioHub; no device, disk audio or network."""
from __future__ import annotations
import json
import struct
import sys

import candidate as common
import dev_check
import lira_ctc


class Stream:
    def __init__(self, classify, verify):
        import numpy as np
        self.np, self.classify, self.verify = np, classify, verify
        self.reset()

    def reset(self):
        self.pcm = self.np.zeros(0, dtype=self.np.float32)
        self.processed, self.until = 0, 0
        self.armed, self.pending = True, None

    def accept(self, values, speech):
        if len(values) != common.STRIDE or not self.np.isfinite(values).all():
            raise ValueError("one ordered 80ms frame required")
        self.pcm = self.np.concatenate((self.pcm, values))[-(common.WINDOW + common.TAIL):]
        self.processed += len(values)
        score = 0.0
        if speech:
            window = self.pcm[-common.WINDOW:]
            if len(window) < common.WINDOW:
                window = self.np.pad(window, (common.WINDOW - len(window), 0))
            score = float(self.classify(window))
        if not 0 <= score <= 1:
            raise ValueError("invalid classifier result")
        if score < common.THRESHOLD:
            self.armed = True
        elif self.armed and self.processed >= self.until:
            self.pending = self.processed
            self.until, self.armed = self.processed + common.COOLDOWN, False
        detected = False
        proposal = self.pending
        if proposal is not None and self.processed >= proposal + common.TAIL:
            start = max(0, proposal - common.WINDOW)
            available_start = self.processed - len(self.pcm)
            if start < available_start:
                raise ValueError("verifier audio no longer available")
            snapshot = self.pcm[start - available_start:proposal + common.TAIL - available_start]
            detected = bool(self.verify(snapshot))
            self.pending = None
        return {"processed": self.processed, "detected": detected}


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
    np, cnn, tone = dev_check.prepare()
    # First-use native/library work belongs to Loading, before opening the mic.
    # Synthetic silence warms sessions only; it is never quality exposure.
    cnn.run(None, {"logmel": common.frontend(np.zeros((1, common.WINDOW), dtype=np.float32)).numpy()})
    tone.score(np.zeros(common.RATE, dtype=np.float32))
    stream = Stream(
        lambda window: cnn.run(None, {"logmel": common.frontend(window[None, :]).numpy()})[0][0][0],
        lambda snapshot: tone.score(snapshot)["greedy_whole_phrase"],
    )
    reply({"protocol": 1, "cnn": dev_check.CNN_SHA256, "verifier": dev_check.POINT_SHA256})
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
        # Only technical failure; never output transcripts, private PCM or paths.
        reply({"error": "worker_failed"})
        sys.exit(1)
