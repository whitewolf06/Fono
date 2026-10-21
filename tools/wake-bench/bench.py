"""File-only WakeWord research runner; no recording, downloads or application IPC."""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import wave

PHRASES = {"ru": "эй фоно", "en": "hey fono"}
POINTS = [
    {"id": "baseline", "stability_ms": 250, "confirmations": 2, "cooldown_ms": 2000},
    {"id": "responsive", "stability_ms": 150, "confirmations": 2, "cooldown_ms": 2000},
    {"id": "conservative", "stability_ms": 400, "confirmations": 3, "cooldown_ms": 2000},
]
MODEL_FILES = {
    "ru": ["model.onnx", "tokens.txt"],
    "en": [f"{part}-epoch-99-avg-1-chunk-16-left-128{suffix}.onnx"
           for part, suffix in [("encoder", ".int8"), ("decoder", ""), ("joiner", ".int8")]] + ["tokens.txt"],
}


def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def load_json(path):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError(f"duplicate JSON key: {key}")
            result[key] = value
        return result
    return json.loads(Path(path).read_text(encoding="utf-8"), object_pairs_hook=pairs,
                      parse_constant=lambda value: (_ for _ in ()).throw(ValueError(f"invalid number: {value}")))


def number(value, minimum=0):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value < minimum:
        raise ValueError("invalid numeric measurement")
    return value


def opaque(value):
    if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z0-9_.-]{1,100}", value):
        raise ValueError("use opaque ASCII IDs, not names")
    return value


def validate_corpus(path):
    manifest = load_json(path)
    if manifest.get("schema") != "fono-wake-corpus@1" or manifest.get("phrases") != PHRASES:
        raise ValueError("expected fono-wake-corpus@1 with fixed RU/EN phrases")
    opaque(manifest.get("id"))
    if not isinstance(manifest.get("entries"), list) or not manifest["entries"]:
        raise ValueError("corpus has no entries")
    ids, hashes, groups, entries = set(), set(), {}, []
    for item in manifest["entries"]:
        item = dict(item)
        identifier = opaque(item.get("id"))
        if identifier in ids:
            raise ValueError("duplicate clip ID")
        ids.add(identifier)
        if item.get("language") not in PHRASES or item.get("split") not in ("train", "dev", "test"):
            raise ValueError("invalid language/split")
        if item.get("origin") not in ("real", "synthetic") or item.get("condition") not in ("quiet", "noisy"):
            raise ValueError("invalid origin/condition")
        if item.get("activity") not in ("idle", "speech"):
            raise ValueError("invalid activity")
        for key in ("license", "source"):
            if not isinstance(item.get(key), str) or not item[key].strip():
                raise ValueError("source and permission/license evidence are required")
        if item.get("permission") not in ("public-license", "consented"):
            raise ValueError("permission evidence is required")
        for key in ("speaker", "session", "source_group"):
            group = (key, opaque(item.get(key)))
            if group in groups and groups[group] != item["split"]:
                raise ValueError(f"split leakage by {key}")
            groups[group] = item["split"]
        wav = (Path(path).resolve().parent / item["wav"]).resolve()
        if wav.stat().st_size > 64 * 1024 * 1024:
            raise ValueError("WAV exceeds 64 MiB; split while retaining its source group")
        digest = sha256(wav)
        if digest != item.get("sha256"):
            raise ValueError(f"WAV hash mismatch: {identifier}")
        if digest in hashes:
            raise ValueError("duplicate audio content, including aliases across splits")
        hashes.add(digest)
        with wave.open(str(wav), "rb") as audio:
            if (audio.getnchannels(), audio.getsampwidth(), audio.getframerate(), audio.getcomptype()) != (1, 2, 16000, "NONE"):
                raise ValueError("expected PCM16 mono 16 kHz WAV")
            frames = audio.getnframes()
            if frames == 0 or len(audio.readframes(frames)) != frames * 2:
                raise ValueError("empty or truncated WAV")
        duration = frames * 1000 / 16000
        annotations = item.get("phrases")
        if not isinstance(annotations, list):
            raise ValueError("phrases must be an explicit annotation list (empty for negatives)")
        previous_end = -1
        for annotation in annotations:
            start, end = number(annotation["start_ms"]), number(annotation["end_ms"])
            if not (previous_end <= start < end <= duration):
                raise ValueError("invalid or overlapping phrase annotation")
            previous_end = end
        if annotations and item["activity"] != "speech":
            raise ValueError("positive annotation must be speech")
        item.update(_wav=wav, duration_ms=duration, audio_samples=frames)
        entries.append(item)
    return manifest, entries


def validate_points(points):
    if not isinstance(points, list) or not 1 <= len(points) <= 9:
        raise ValueError("expect 1..9 explicit operating points")
    ids = set()
    for point in points:
        identifier = opaque(point.get("id"))
        if identifier in ids or set(point) != {"id", "stability_ms", "confirmations", "cooldown_ms"}:
            raise ValueError("duplicate point or unexpected fields")
        ids.add(identifier)
        for key, lower, upper in (("stability_ms", 0, 3000), ("confirmations", 2, 10), ("cooldown_ms", 0, 10000)):
            value = number(point[key], lower)
            if not isinstance(value, int) or value > upper:
                raise ValueError("invalid operating point")
    return points


def validate_trace(trace, item, realtime):
    if trace.get("schema") != 1 or trace.get("sample_rate") != 16000 or trace.get("realtime") is not realtime:
        raise ValueError("replay trace contract mismatch")
    if trace.get("audio_samples") != item["audio_samples"] or abs(number(trace["audio_duration_ms"]) - item["duration_ms"]) > 0.001:
        raise ValueError("replay trace has different or truncated audio")
    for key in ("model_load_ms", "replay_wall_ms"):
        number(trace[key])
    if realtime and trace["replay_wall_ms"] + 1 < item["duration_ms"]:
        raise ValueError("paced replay finished before its audio clock")
    if trace.get("process_cpu_ms") is not None:
        number(trace["process_cpu_ms"])
    previous_audio = previous_wall = -1
    for event in trace["events"]:
        audio, wall = number(event["audio_ms"]), number(event["delivered_ms"])
        if not (previous_audio <= audio <= item["duration_ms"] and previous_wall <= wall <= trace["replay_wall_ms"] + 1):
            raise ValueError("invalid detection timeline")
        if realtime and wall + 1 < audio:
            raise ValueError("paced event precedes available audio")
        previous_audio, previous_wall = audio, wall


def quantile(values, percentile):
    return sorted(values)[max(0, math.ceil(len(values) * percentile) - 1)] if values else None


def score(item, trace):
    """One delivery can match one annotation; keep signed end-to-delivery latency."""
    unmatched = set(range(len(item["phrases"])))
    latency, sample_latency, false_events = [], [], 0
    for event in trace["events"]:
        possible = [i for i in unmatched if item["phrases"][i]["start_ms"] <= event["audio_ms"] <= item["phrases"][i]["end_ms"] + 2000]
        if not possible:
            false_events += 1
            continue
        index = min(possible, key=lambda i: abs(event["audio_ms"] - item["phrases"][i]["end_ms"]))
        unmatched.remove(index)
        end = item["phrases"][index]["end_ms"]
        sample_latency.append(event["audio_ms"] - end)
        if trace["realtime"]:
            latency.append(event["delivered_ms"] - end)
    return {"matched": len(item["phrases"]) - len(unmatched), "false_events": false_events,
            "latency_ms": latency, "sample_clock_latency_ms": sample_latency}


def coverage(entries):
    positives = [e for e in entries if e["phrases"] and e["origin"] == "real"]
    count = sum(len(e["phrases"]) for e in positives)
    hours = sum(e["duration_ms"] for e in entries if not e["phrases"] and e["origin"] == "real") / 3600000
    missing = []
    if count < 100:
        missing.append("100 real annotated phrase utterances")
    if hours < 10:
        missing.append("10 hours of real negatives")
    if len({e["speaker"] for e in positives}) < 2:
        missing.append("different real positive speakers")
    for language in PHRASES:
        for condition in ("quiet", "noisy"):
            if not any(e["language"] == language and e["condition"] == condition for e in positives):
                missing.append(f"real {language}/{condition} positives")
        if not any(e["language"] == language and not e["phrases"] and e["origin"] == "real" for e in entries):
            missing.append(f"real {language} negatives")
    return {"real_positives": count, "real_negative_hours": hours, "missing": missing, "eligible": not missing}


def aggregate(rows):
    recall = {}
    for condition in ("quiet", "noisy"):
        subset = [r for r in rows if r["condition"] == condition and r["origin"] == "real"]
        total = sum(r["positives"] for r in subset)
        recall[condition] = sum(r["matched"] for r in subset) / total if total else None
    negative = [r for r in rows if not r["positives"] and r["origin"] == "real"]
    hours = sum(r["duration_ms"] for r in negative) / 3600000
    false_events = sum(r["false_events"] for r in negative)
    delays = [v for r in rows if r["origin"] == "real" for v in r["latency_ms"]]
    cpu = {}
    for activity in ("idle", "speech"):
        subset = [r for r in rows if r["activity"] == activity]
        measured = subset and all(r["trace"]["realtime"] and r["trace"]["process_cpu_ms"] is not None for r in subset)
        wall = sum(r["trace"]["replay_wall_ms"] for r in subset)
        cpu[activity] = sum(r["trace"]["process_cpu_ms"] for r in subset) / wall * 100 if measured and wall else None
    return {"recall": recall, "negative_hours": hours, "false_activations": false_events,
            "false_activations_per_hour": false_events / hours if hours else None,
            "latency_p50_ms": quantile(delays, .50), "latency_p95_ms": quantile(delays, .95),
            "cpu_percent_one_core": cpu}


def criteria(metrics):
    values = [("quiet_recall", metrics["recall"]["quiet"], lambda v: v >= .95),
              ("noisy_recall", metrics["recall"]["noisy"], lambda v: v >= .90),
              ("false_activations_per_hour", metrics["false_activations_per_hour"], lambda v: v <= .5),
              ("latency_p95_ms", metrics["latency_p95_ms"], lambda v: v <= 800),
              ("idle_cpu_one_core_percent", metrics["cpu_percent_one_core"]["idle"], lambda v: v < 1)]
    return {key: "not_measured" if value is None else "passed" if check(value) else "failed" for key, value, check in values}


def verify_selection(selection, lineage, point):
    if selection["run"]["split"] != "dev" or selection["run"]["smoke"]:
        raise ValueError("test selection must come from a real dev run")
    if selection["lineage"] != lineage or point not in [p["point"] for p in selection["points"]]:
        raise ValueError("selection lineage/operating point changed")


def run(args):
    manifest, entries = validate_corpus(args.corpus)
    selected = [e for e in entries if e["split"] == args.split]
    if not selected:
        raise ValueError("selected split is empty")
    points = validate_points(load_json(args.points) if args.points else POINTS)
    if args.point:
        points = [p for p in points if p["id"] == args.point]
        if not points:
            raise ValueError("unknown selected point")
    if args.split == "test" and (len(points) != 1 or not (args.selection or args.smoke)):
        raise ValueError("test requires one frozen dev point/selection; use --smoke for engineering checks")
    models = {language: Path(getattr(args, f"model_{language}")).resolve() for language in {e["language"] for e in selected}}
    model_hashes = {language: {name: sha256(directory / name) for name in MODEL_FILES[language]} for language, directory in models.items()}
    runtime = {name: sha256(Path(args.replay).resolve().parent / name)
               for name in ("onnxruntime.dll", "onnxruntime_providers_shared.dll", "sherpa-onnx-c-api.dll", "sherpa-onnx-cxx-api.dll")
               if (Path(args.replay).resolve().parent / name).is_file()}
    lineage = {"corpus_sha256": sha256(args.corpus), "binary_sha256": sha256(args.replay),
               "runner_sha256": sha256(__file__), "runtime": runtime, "models": model_hashes,
               "hardware": {"processor": platform.processor(), "machine": platform.machine(), "logical_cpus": os.cpu_count(), "system": platform.system(), "release": platform.release()},
               "realtime": args.mode == "realtime"}
    if args.split == "test" and not args.smoke:
        verify_selection(load_json(args.selection), lineage, points[0])
    result = {"schema": "fono-wake-bench@1", "corpus": manifest["id"], "lineage": lineage,
              "run": {"split": args.split, "mode": args.mode, "smoke": args.smoke},
              "coverage": coverage(selected), "points": [], "qualified": False}
    for point in points:
        rows = []
        for item in selected:
            request = {"backend": item["language"], "phrase": PHRASES[item["language"]], "model_dir": str(models[item["language"]]),
                       "wav": str(item["_wav"]), "realtime": args.mode == "realtime", **{k: point[k] for k in ("stability_ms", "confirmations", "cooldown_ms")}}
            completed = subprocess.run([str(Path(args.replay).resolve())], input=json.dumps(request, ensure_ascii=False),
                                       text=True, encoding="utf-8", capture_output=True,
                                       timeout=max(90, item["duration_ms"] / 1000 * (1.5 if args.mode == "realtime" else 5) + 30), check=False)
            if completed.returncode:
                raise ValueError(f"replay failed for {item['id']} (exit {completed.returncode}); no report accepted")
            trace = json.loads(completed.stdout)
            validate_trace(trace, item, args.mode == "realtime")
            if sha256(item["_wav"]) != item["sha256"]:
                raise ValueError("audio changed during replay")
            rows.append({**{k: item[k] for k in ("id", "sha256", "language", "condition", "origin", "activity", "duration_ms")},
                         "positives": len(item["phrases"]), "trace": trace, **score(item, trace)})
        metrics = aggregate(rows)
        gates = criteria(metrics)
        result["points"].append({"point": point, "metrics": metrics, "criteria": gates, "entries": rows})
    result["tradeoff"] = [{"point": p["point"]["id"], "false_activations_per_hour": p["metrics"]["false_activations_per_hour"],
                           "miss_quiet": None if p["metrics"]["recall"]["quiet"] is None else 1 - p["metrics"]["recall"]["quiet"],
                           "miss_noisy": None if p["metrics"]["recall"]["noisy"] is None else 1 - p["metrics"]["recall"]["noisy"]} for p in result["points"]]
    result["curve_kind"] = "discrete ASR configuration sweep; no classifier probability score"
    result["qualified"] = bool(args.split == "test" and not args.smoke and result["coverage"]["eligible"] and
                               all(v == "passed" for v in result["points"][0]["criteria"].values()))
    result["quality_status"] = "passed" if result["qualified"] else "insufficient_data" if not result["coverage"]["eligible"] or args.smoke else "not_measured" if any(v == "not_measured" for p in result["points"] for v in p["criteria"].values()) else "not_met"
    if sha256(args.corpus) != lineage["corpus_sha256"] or sha256(args.replay) != lineage["binary_sha256"] or sha256(__file__) != lineage["runner_sha256"]:
        raise ValueError("benchmark lineage changed during replay")
    if any(sha256(models[language] / name) != digest for language, files in model_hashes.items() for name, digest in files.items()):
        raise ValueError("model changed during replay")
    if any(sha256(Path(args.replay).resolve().parent / name) != digest for name, digest in runtime.items()):
        raise ValueError("runtime changed during replay")
    # Paths, personal IDs, raw transcripts, phrase audio and decoder stderr stay out.
    Path(args.output).parent.mkdir(parents=True, exist_ok=True)
    Path(args.output).write_text(json.dumps(result, ensure_ascii=False, indent=2, allow_nan=False) + "\n", encoding="utf-8")
    print(json.dumps({"quality_status": result["quality_status"], "qualified": result["qualified"], "coverage": result["coverage"], "points": [{"point": p["point"], "metrics": p["metrics"], "criteria": p["criteria"]} for p in result["points"]]}, ensure_ascii=False))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    validate = sub.add_parser("validate")
    validate.add_argument("--corpus", required=True)
    execute = sub.add_parser("run")
    execute.add_argument("--corpus", required=True)
    execute.add_argument("--replay", required=True)
    execute.add_argument("--model-ru", default="")
    execute.add_argument("--model-en", default="")
    execute.add_argument("--output", required=True)
    execute.add_argument("--split", choices=("dev", "test"), default="dev")
    execute.add_argument("--mode", choices=("fast", "realtime"), default="fast")
    execute.add_argument("--points")
    execute.add_argument("--point")
    execute.add_argument("--selection")
    execute.add_argument("--smoke", action="store_true")
    args = parser.parse_args()
    try:
        if args.command == "validate":
            manifest, entries = validate_corpus(args.corpus)
            print(json.dumps({"corpus": manifest["id"], "splits": {split: coverage([e for e in entries if e["split"] == split]) for split in ("train", "dev", "test")}}, ensure_ascii=False))
        else:
            run(args)
    except (ValueError, KeyError, OSError, wave.Error, subprocess.TimeoutExpired) as error:
        parser.exit(2, f"wake-bench: {error}\n")


if __name__ == "__main__":
    main()
