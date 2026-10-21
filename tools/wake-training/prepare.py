"""Prepare local, split-isolated training WAVs. No training, network or capture."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import io
import json
import math
from pathlib import Path
import sys
import wave

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("fono_wake_bench", HERE.parent / "wake-bench" / "bench.py")
bench = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bench)
RATE = 16000
WINDOW = 2 * RATE


def configs():
    """JSON is also YAML: upstream load_config uses yaml.safe_load."""
    result = {}
    for language in bench.PHRASES:
        config = bench.load_json(HERE / f"{language}.json")
        if config["model_name"] != f"fono_{language}" or config["target_phrases"] != [bench.PHRASES[language]]:
            raise ValueError("training config must use the fixed language/phrase")
        if config["augmentation"]["clip_duration"] != WINDOW / RATE:
            raise ValueError("training window/config mismatch")
        result[language] = config
    return result


def inspect(corpus):
    before = bench.sha256(corpus)
    manifest, entries = bench.validate_corpus(corpus)
    counts = {language: {split: {"positive": 0, "negative": 0, "background": 0}
                         for split in ("train", "dev")} for language in bench.PHRASES}
    for entry in entries:
        if entry["split"] == "test":
            continue
        group = counts[entry["language"]][entry["split"]]
        if entry["phrases"]:
            for annotation in entry["phrases"]:
                start = math.floor(annotation["start_ms"] * RATE / 1000)
                end = math.ceil(annotation["end_ms"] * RATE / 1000)
                if end - start > WINDOW:
                    raise ValueError("positive phrase exceeds 2s; do not silently truncate it")
                group["positive"] += 1
        else:
            kind = "background" if entry["activity"] == "idle" else "negative"
            group[kind] += math.ceil(entry["audio_samples"] / WINDOW)
    if bench.sha256(corpus) != before:
        raise ValueError("corpus changed during validation")
    missing = [f"{language}/{split}/{kind}" for language, splits in counts.items()
               for split, classes in splits.items() for kind in ("positive", "negative")
               if not classes[kind]]
    report = {"schema": "fono-wake-training-preparation@1", "corpus_id": manifest["id"],
              "corpus_sha256": before, "counts": counts, "missing": missing,
              "data_ready": not missing, "training_started": False, "qualified": False,
              "excluded_test_clips": sum(e["split"] == "test" for e in entries)}
    return report, entries


def write_json(path, data):
    Path(path).write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def prepare(corpus, destination):
    settings = configs()
    report, entries = inspect(corpus)
    if not report["data_ready"]:
        raise ValueError("missing training/development classes: " + ", ".join(report["missing"]))
    root = Path(destination).resolve()
    root.mkdir(parents=True, exist_ok=False)
    # This directory is intentionally fresh. A failure never writes the final manifest.
    artifacts = []
    for index, entry in enumerate(entries):
        if entry["split"] == "test":
            continue
        raw = entry["_wav"].read_bytes()
        if hashlib.sha256(raw).hexdigest() != entry["sha256"]:
            raise ValueError("WAV changed since validation")
        with wave.open(io.BytesIO(raw), "rb") as audio:
            samples = audio.readframes(audio.getnframes())
        positive = bool(entry["phrases"])
        kind = "positive" if positive else "background" if entry["activity"] == "idle" else "negative"
        # Upstream calls its validation directories *_test. Only Fono dev goes there.
        split = "train" if entry["split"] == "train" else "test"
        folder = root / "output" / f"fono_{entry['language']}" / f"{kind}_{split}"
        folder.mkdir(parents=True, exist_ok=True)
        bounds = [(math.floor(a["start_ms"] * RATE / 1000), math.ceil(a["end_ms"] * RATE / 1000))
                  for a in entry["phrases"]] if positive else [
                      (start, min(start + WINDOW, entry["audio_samples"]))
                      for start in range(0, entry["audio_samples"], WINDOW)]
        for window, (start, end) in enumerate(bounds):
            target = folder / f"clip_{index:06d}_{window:06d}.wav"
            with wave.open(str(target), "wb") as audio:
                audio.setparams((1, 2, RATE, 0, "NONE", "not compressed"))
                audio.writeframes(samples[start * 2:end * 2])
            artifacts.append({"file": target.relative_to(root).as_posix(), "sha256": bench.sha256(target),
                              "source_id": entry["id"], "source_sha256": entry["sha256"],
                              "source_split": entry["split"], "source_start_sample": start,
                              "source_end_sample": end})
    for language, config in settings.items():
        # Relative paths are rooted in the prepared directory, not the repository.
        write_json(root / f"{language}.json", config)
    if bench.sha256(corpus) != report["corpus_sha256"]:
        raise ValueError("corpus changed during preparation")
    report.update(prepared=True, artifacts=artifacts, script_sha256=bench.sha256(__file__),
                  bench_script_sha256=bench.sha256(HERE.parent / "wake-bench" / "bench.py"),
                  configs_sha256={lang: bench.sha256(root / f"{lang}.json") for lang in settings})
    # No paths, source text, speaker names or license declarations in a shareable report.
    write_json(root / "preparation.json", report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("inspect", "prepare"))
    parser.add_argument("--corpus", required=True, type=Path)
    parser.add_argument("--output-dir", type=Path)
    args = parser.parse_args()
    if args.action == "prepare" and args.output_dir is None:
        parser.error("prepare requires --output-dir (a new directory)")
    try:
        report = prepare(args.corpus, args.output_dir) if args.action == "prepare" else inspect(args.corpus)[0]
        summary = {key: value for key, value in report.items() if key != "artifacts"}
        print(json.dumps(summary, ensure_ascii=False, indent=2))
        return 0 if report["data_ready"] else 2
    except (ValueError, OSError, KeyError, TypeError, EOFError, wave.Error) as error:
        # Raw OS errors can contain private paths; keep command output safe to paste.
        print(f"preparation rejected ({type(error).__name__}); verify local corpus/config and a fresh destination", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
