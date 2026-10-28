"""Optional Vosk competitor: same corpus JSON and exact stable phrase policy.

Install vosk==0.3.45 in an isolated environment; this is not a Fono dependency.
Usage: python vosk_replay.py MODEL_DIR PHRASE CORPUS_JSON [--grammar]
No microphone, audio uploads or transcript persistence are used.
"""
import argparse
import audioop
import json
import math
import re
import time
import wave
from collections import deque
from pathlib import Path

from vosk import KaldiRecognizer, Model, SetLogLevel


def normalize(text):
    return " ".join(re.sub(r"[^\w\s]", " ", text.lower().replace("ё", "е")).split())


class Session:
    def __init__(self, model, phrase, grammar, stability_ms):
        self.model, self.phrase, self.grammar = model, normalize(phrase), grammar
        self.stability_samples = stability_ms * 16
        self.retained = deque()
        self.retained_frames = 0
        self.origin = None
        self.candidate = None
        self.reset_recognizer()

    def reset_recognizer(self):
        if self.grammar:
            self.rec = KaldiRecognizer(self.model, 16000, json.dumps([self.phrase, "[unk]"], ensure_ascii=False))
        else:
            self.rec = KaldiRecognizer(self.model, 16000)
        self.rec.SetWords(True)
        self.candidate = None

    def observe(self, raw, clock):
        result = json.loads(raw)
        text = normalize(result.get("partial", result.get("text", "")))
        match = re.search(r"(?<!\w)" + re.escape(self.phrase) + r"(?!\w)", text)
        if not match:
            self.candidate = None
            return False
        if self.candidate is None or self.candidate[0] != match.start():
            self.candidate = (match.start(), clock, 1)
        else:
            start, since, count = self.candidate
            self.candidate = (start, since, count + 1)
        return self.candidate[2] >= 2 and clock - self.candidate[1] >= self.stability_samples

    def decode(self, raw, clock):
        endpoint = self.rec.AcceptWaveform(raw)
        result = self.rec.Result() if endpoint else self.rec.PartialResult()
        detected = self.observe(result, clock)
        if endpoint:
            self.candidate = None
        return detected, endpoint

    def accept(self, raw, end):
        frames = len(raw) // 2
        if self.origin is None:
            self.origin = end - frames
        self.retained.append((raw, end - frames, end))
        self.retained_frames += frames
        while self.retained_frames > 32000 and len(self.retained) > 1:
            self.retained_frames -= len(self.retained.popleft()[0]) // 2
        detected, endpoint = self.decode(raw, end)
        if not detected and end - self.origin >= 320000:
            self.reset_recognizer()
            self.origin = self.retained[0][1]
            for buffered, _, clock in self.retained:
                replayed, _ = self.decode(buffered, clock)
                detected |= replayed
        elif endpoint:
            self.origin = None
            self.retained.clear()
            self.retained_frames = 0
        if detected:
            self.reset_recognizer()
            self.origin = None
            self.retained.clear()
            self.retained_frames = 0
        return detected


def replay(model, args, item):
    with wave.open(str(item["wav"]), "rb") as wav:
        if wav.getnchannels() != 1 or wav.getsampwidth() != 2 or wav.getcomptype() != "NONE":
            raise ValueError("corpus WAV must be mono 16-bit PCM")
        rate, frames = wav.getframerate(), wav.getnframes()
        raw = wav.readframes(frames)
    if rate != 16000:
        raw, _ = audioop.ratecv(raw, 2, 1, rate, 16000, None)
    raw += bytes(32000)  # Same one-second final silence as Sherpa replay.
    session = Session(model, args.phrase, args.grammar, args.stability_ms)
    clock, cooldown_until, detections, first = 0, 0, 0, None
    started = time.perf_counter()
    for offset in range(0, len(raw), 640):
        chunk = raw[offset:offset + 640]
        clock += len(chunk) // 2
        if session.accept(chunk, clock) and clock >= cooldown_until:
            detections += 1
            first = first if first is not None else clock // 16
            cooldown_until = clock + 32000
    return {"detections": detections, "first_ms": first, "duration_ms": frames * 1000 // rate,
            "processing_ms": round((time.perf_counter() - started) * 1000)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("model_dir")
    parser.add_argument("phrase")
    parser.add_argument("corpus")
    parser.add_argument("--grammar", action="store_true")
    parser.add_argument("--stability-ms", type=int, default=250)
    args = parser.parse_args()
    SetLogLevel(-1)
    started = time.perf_counter()
    model = Model(args.model_dir)
    report = {"backend": "vosk-0.3.45", "model": Path(args.model_dir).name,
              "grammar": args.grammar, "model_load_ms": round((time.perf_counter() - started) * 1000),
              "positives": 0, "positives_detected": 0, "holdout_positives": 0,
              "holdout_detected": 0, "negative_hours": 0, "false_detections": 0,
              "false_detections_per_hour": None, "p95_delay_ms": None, "processing_ms": 0}
    delays = []
    for item in json.loads(Path(args.corpus).read_text(encoding="utf-8-sig")):
        result = replay(model, args, item)
        report["processing_ms"] += result["processing_ms"]
        if item["positive"]:
            report["positives"] += 1
            report["positives_detected"] += int(result["detections"] > 0)
            if item.get("holdout", False):
                report["holdout_positives"] += 1
                report["holdout_detected"] += int(result["detections"] > 0)
            if result["first_ms"] is not None and item.get("phrase_end_ms") is not None:
                delays.append(max(0, result["first_ms"] - item["phrase_end_ms"]))
        else:
            report["negative_hours"] += result["duration_ms"] / 3600000
            report["false_detections"] += result["detections"]
    if report["negative_hours"]:
        report["false_detections_per_hour"] = report["false_detections"] / report["negative_hours"]
    if delays:
        report["p95_delay_ms"] = sorted(delays)[math.ceil(len(delays) * 0.95) - 1]
    print(json.dumps(report, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
