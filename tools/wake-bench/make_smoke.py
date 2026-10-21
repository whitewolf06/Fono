"""Create engineering inputs from the existing licensed FLEURS smoke fixtures."""
import argparse
import json
from pathlib import Path
import wave
from bench import PHRASES, sha256


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    public = root / "src-tauri/tests/fixtures/stt-quality"
    source = json.loads((public / "manifest.json").read_text(encoding="utf-8"))
    if source["license"] != "CC-BY-4.0":
        raise ValueError("existing corpus license changed")
    destination = Path(args.output_dir).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    entries = []
    for language, extra_frames in (("ru", 0), ("en", 320)):
        item = next(e for e in source["entries"] if e["id"] == f"{language}-01")
        audio = public / item["path"]
        if sha256(audio) != item["sha256"] or PHRASES[language] in item["reference"].casefold():
            raise ValueError("public negative fixture changed")
        common = {"language": language, "split": "test", "condition": "quiet", "phrases": [], "permission": "public-license"}
        entries.append({**common, "id": item["id"], "wav": str(audio), "sha256": item["sha256"],
                        "origin": "real", "activity": "speech", "speaker": f"fleurs-{language}-unknown",
                        "session": item["id"], "source_group": item["id"], "license": source["license"], "source": source["source_card"]})
        silence = destination / f"idle-{language}.wav"
        with wave.open(str(silence), "wb") as output:
            output.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
            output.writeframes(bytes((128000 + extra_frames) * 2))
        entries.append({**common, "id": f"idle-{language}", "wav": silence.name, "sha256": sha256(silence),
                        "origin": "synthetic", "activity": "idle", "speaker": "generated", "session": "generated", "source_group": "generated",
                        "license": "Generated zero PCM; no recorded voice", "source": "tools/wake-bench/make_smoke.py"})
    manifest = {"schema": "fono-wake-corpus@1", "id": "fono-wake-engineering-smoke", "phrases": PHRASES, "entries": entries}
    (destination / "corpus.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print("Created 2 public speech negatives + 2 synthetic idle inputs; no positives or quality qualification")


if __name__ == "__main__":
    main()
