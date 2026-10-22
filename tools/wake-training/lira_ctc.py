"""Bounded phonetic verification of the frozen Lira CNN; private recordings are evaluation-only."""
from __future__ import annotations
import argparse
import hashlib
import importlib.metadata
import json
import math
from pathlib import Path
import time

import candidate as common
import lira

ARCHIVE_SHA256 = "b9c907450e99a6e5049e279bf18368a17db0bdc5e63b7fa978943138debbe3ae"
DIRECTORY = "sherpa-onnx-streaming-t-one-russian-2025-09-08"
# Validated byte-for-byte against ARCHIVE_SHA256 in the first calibration.
# Direct pins avoid repeatedly decompressing the 144 MB model on every verifier.
FILE_SHA256 = {
    "model.onnx": "5ded080e2a6c86ecc11bcb0902d77524eb3e8b0844cb0c0754347f5aafb4dabc",
    "tokens.txt": "27f7b3ba2096c572375fba1a6b29af1f80d86e08a329940612908112695f97e0",
    "LICENSE": "c8e6ecb86af681d9815d1ada7b1a7780ea5e5cb68a5df94fc800adab1a6ce027",
}
TARGET = "привет лира"
DECOYS = ("привет лера", "привет мира", "привет ира", "айфон")
CONTEXT = 4 * common.RATE
LOOKAHEAD = common.TAIL


def spot_score(logp, tokens, blank):
    """Best CTC substring alignment, penalized against the unconstrained frame path.

    Free filler may precede/follow the complete target, but never its interior.
    Repeated letters require an intervening blank. Scores are nats per character;
    a score of zero follows a frame-best acoustic path, not a probability of one.
    """
    if not tokens or blank in tokens or not len(logp):
        raise ValueError("a nonempty nonblank target and frames are required")
    vocabulary = len(logp[0])
    if any(not 0 <= x < vocabulary for x in [blank, *tokens]):
        raise ValueError("CTC token outside vocabulary")
    states = [blank]
    for token in tokens:
        states.extend((token, blank))
    previous = [-math.inf] * len(states)
    previous[0] = 0.0
    best = -math.inf
    for row in logp:
        if len(row) != vocabulary or any(not math.isfinite(float(x)) for x in row):
            raise ValueError("invalid CTC emissions")
        maximum = max(row)
        current = [-math.inf] * len(states)
        current[0] = 0.0  # A fresh phrase can begin after arbitrary preceding speech.
        for index in range(1, len(states)):
            sources = [previous[index], previous[index - 1]]
            if index > 1 and states[index] != blank and states[index] != states[index - 2]:
                sources.append(previous[index - 2])
            current[index] = max(sources) + float(row[states[index]]) - float(maximum)
        best = max(best, current[-2])
        previous = current
    return best / len(tokens)


def choose_cutoff(rows):
    """One analytic cutoff from nonprivate calibration, never a private threshold sweep."""
    positives = [r["target_score"] for r in rows if r["positive"]]
    negatives = [r["target_score"] for r in rows if not r["positive"] and r["margin"] > 0]
    if not positives or not negatives or not all(math.isfinite(x) for x in positives + negatives):
        raise ValueError("finite positive and eligible negative calibration required")
    low, high = max(negatives), min(positives)
    if low >= high:
        raise ValueError("calibration has no separating cutoff; private evaluation must not proceed")
    return (low + high) / 2


def accepted(row, cutoff):
    return row["target_score"] >= cutoff and row["margin"] > 0


def whole_phrase(text):
    words = " ".join(text.casefold().replace("ё", "е").split())
    return f" {TARGET} " in f" {words} "


def greedy_text(frames, vocabulary, blank):
    """Ordinary CTC collapse, without forcing the target into another utterance."""
    inverse = {identifier: token for token, identifier in vocabulary.items()}
    result, previous = [], None
    for frame in frames:
        token = max(range(len(frame)), key=lambda i: frame[i])
        if token != blank and token != previous:
            result.append(inverse[token])
        previous = token
    return "".join(result)


def digest_file(path):
    result = hashlib.sha256()
    with Path(path).open("rb") as source:
        for block in iter(lambda: source.read(4 * 1024**2), b""):
            result.update(block)
    return result.hexdigest()


class Tone:
    def __init__(self, model_dir, archive):
        import numpy as np
        import onnxruntime as ort
        root = Path(model_dir)
        if digest_file(archive) != ARCHIVE_SHA256:
            raise ValueError("pinned T-one archive mismatch")
        self.identity = {}
        for name, expected in FILE_SHA256.items():
            path = root / name
            if path.is_symlink() or path.is_junction() or not path.is_file():
                raise ValueError("T-one assets must be regular files")
            self.identity[name] = digest_file(path)
            if self.identity[name] != expected:
                raise ValueError("extracted T-one asset hash mismatch")
        self.vocabulary = {}
        for line in (root / "tokens.txt").read_text(encoding="utf-8").splitlines():
            fields = line.strip().split()
            token, identifier = (" ", int(fields[0])) if len(fields) == 1 else (fields[0], int(fields[1]))
            self.vocabulary[token] = identifier
        self.blank = self.vocabulary["<blk>"]
        self.targets = {text: [self.vocabulary[c] for c in text] for text in (TARGET, *DECOYS)}
        options = ort.SessionOptions()
        options.intra_op_num_threads, options.inter_op_num_threads = 2, 1
        options.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
        self.session = ort.InferenceSession(str(root / "model.onnx"), options, providers=["CPUExecutionProvider"])
        metadata = self.session.get_modelmeta().custom_metadata_map
        if (metadata.get("model_type") != "t-one" or metadata.get("sample_rate") != "8000"
                or metadata.get("state_dim") != "219729" or metadata.get("frame_length_ms") != "300"
                or [x.name for x in self.session.get_inputs()] != ["signal", "state"]
                or [x.name for x in self.session.get_outputs()] != ["logprobs", "state_next"]):
            raise ValueError("unexpected T-one tensor contract")
        self.np = np

    def score(self, pcm):
        np = self.np
        from scipy.signal import resample_poly
        if not 1 <= len(pcm) <= 90 * common.RATE or not np.isfinite(pcm).all():
            raise ValueError("invalid bounded verifier PCM")
        signal = resample_poly(pcm, 1, 2, window=("kaiser", 5.0)).astype(np.float32)
        # Upstream raw-audio reference has a leading chunk; trailing chunks flush
        # the model explicitly. These zeros are not real negative exposure.
        signal = np.pad(signal, (2400, 4800))
        signal = np.pad(signal, (0, (-len(signal)) % 2400))
        signal = (np.clip(signal, -1, 32767 / 32768) * 32768).astype(np.int32)
        state = np.zeros((1, 219729), dtype=np.float16)
        frames = []
        for chunk in signal.reshape(-1, 2400):
            logp, state = self.session.run(None, {"signal": chunk.reshape(1, 2400, 1), "state": state})
            if logp.shape != (1, 10, 35) or state.shape != (1, 219729):
                raise ValueError("invalid streaming T-one output")
            frames.extend(logp[0].tolist())
        scores = {text: spot_score(frames, tokens, self.blank) for text, tokens in self.targets.items()}
        return {"target_score": scores[TARGET], "margin": scores[TARGET] - max(scores[t] for t in DECOYS),
                "greedy_whole_phrase": whole_phrase(greedy_text(frames, self.vocabulary, self.blank)),
                "decoy_scores": {t: scores[t] for t in DECOYS}}


def calibrate(args):
    synthetic = lira.training_sources(args.training_manifest)
    public = lira.public_sources(args.public_speech)
    tone = Tone(args.model_dir, args.archive)
    output = Path(args.output_dir)
    output.mkdir(parents=True, exist_ok=False)
    started = time.monotonic()
    rows = []
    for index, (metadata, pcm) in enumerate(synthetic):
        if time.monotonic() - started > 180:
            raise TimeoutError("single calibration budget exhausted")
        rows.append({"source": "synthetic", "index": index, "positive": metadata["positive"], **tone.score(pcm)})
    for index, pcm in enumerate(public):
        if time.monotonic() - started > 180:
            raise TimeoutError("single calibration budget exhausted")
        rows.append({"source": "public", "index": index, "positive": False, **tone.score(pcm)})
    common.write_json(output / "calibration-scores.json", rows)
    cutoff = choose_cutoff(rows)
    config = {"schema": "fono-lira-ctc-verifier@1", "target": TARGET, "decoys": DECOYS,
              "cutoff": cutoff, "required_margin": 0.0, "cnn_threshold": common.THRESHOLD,
              "context_samples": CONTEXT, "lookahead_samples": LOOKAHEAD,
              "source_sha256": digest_file(__file__), "model_identity": tone.identity,
              "archive_sha256": ARCHIVE_SHA256, "private_voice_used_for_calibration": False,
              "synthetic_manifest_sha256": digest_file(args.training_manifest),
              "public_manifest_sha256": digest_file(args.public_speech),
              "versions": {n: importlib.metadata.version(n) for n in ("numpy", "scipy", "onnxruntime")},
              "calibration": {"positive_clips": sum(r["positive"] for r in rows),
                              "positive_accepted": sum(r["positive"] and accepted(r, cutoff) for r in rows),
                              "negative_clips": sum(not r["positive"] for r in rows),
                              "negative_accepted": sum(not r["positive"] and accepted(r, cutoff) for r in rows),
                              "wall_seconds": round(time.monotonic() - started, 3)},
              "qualified": False}
    common.write_json(output / "config.json", config)
    print(json.dumps(config, ensure_ascii=False, indent=2))


def prepare_exact(args):
    prior_path = Path(args.config_dir) / "config.json"
    prior = json.loads(prior_path.read_text(encoding="utf-8"))
    if (prior["archive_sha256"] != ARCHIVE_SHA256 or prior["target"] != TARGET
            or prior["private_voice_used_for_calibration"] is not False):
        raise ValueError("unexpected predecessor model/configuration")
    config = {**prior, "source_sha256": digest_file(__file__), "decision_rule": "greedy_whole_phrase",
              "cutoff": None, "prior_cost_config_sha256": digest_file(prior_path),
              "prior_cost_calibration": prior["calibration"],
              "calibration": {"kind": "no fitting; exact whole-word frame-best CTC sequence required"},
              "selected_after_dev_feedback": True, "independent_test_claimed": False}
    output = Path(args.output_dir)
    output.mkdir(parents=True, exist_ok=False)
    common.write_json(output / "config.json", config)
    print(json.dumps({"decision_rule": config["decision_rule"], "cutoff": None,
                      "new_training_runs": 0, "new_calibration_inference_runs": 0}))


def evaluate(args):
    import numpy as np
    root = Path(args.config_dir)
    config = json.loads((root / "config.json").read_text(encoding="utf-8"))
    rule = config.get("decision_rule", "calibrated_cost")
    if (config["source_sha256"] != digest_file(__file__) or config["target"] != TARGET
            or config["decoys"] != list(DECOYS) or config["required_margin"] != 0.0
            or config["private_voice_used_for_calibration"] is not False
            or config["cnn_threshold"] != common.THRESHOLD
            or config["context_samples"] != CONTEXT or config["lookahead_samples"] != LOOKAHEAD
            or rule not in ("calibrated_cost", "greedy_whole_phrase")):
        raise ValueError("frozen verifier source or operating point changed")
    tone = Tone(args.model_dir, args.archive)
    if tone.identity != config["model_identity"]:
        raise ValueError("verifier model changed after calibration")
    baseline = json.loads(Path(args.baseline_traces).read_text(encoding="utf-8"))
    previous = {r["id"]: r for r in baseline}
    records = [r for r in lira.recordings.load_recordings(args.recordings_dir) if r["phrase_id"] == lira.PHRASE_ID]
    if set(previous) != {r["sample"]["id"] for r in records}:
        raise ValueError("paired private recording set changed")
    output = Path(args.output_dir)
    output.mkdir(parents=True, exist_ok=False)
    started = time.monotonic()
    groups, traces = {}, []
    for row in records:
        if time.monotonic() - started > 120:
            raise TimeoutError("single paired evaluation budget exhausted")
        sample = row["sample"]
        old = previous[sample["id"]]
        if sample["sha256"] != old["sha256"]:
            raise ValueError("paired private WAV changed")
        pcm = np.frombuffer(row["pcm"], dtype="<i2").astype(np.float32) / 32768
        events = []
        for event in old["events_samples"]:
            end = min(len(pcm), event + LOOKAHEAD)
            begin = max(0, event - CONTEXT)
            score = tone.score(pcm[begin:end])
            events.append({"proposal_sample": event, "available_sample": event + LOOKAHEAD,
                           **score, "accepted": (score["greedy_whole_phrase"] if rule == "greedy_whole_phrase"
                                                 else accepted(score, config["cutoff"]))})
        hit = any(e["accepted"] for e in events)
        key = (sample["kind"], sample["condition"])
        group = groups.setdefault(key, {"kind": key[0], "condition": key[1], "clips": 0,
                                        "baseline_hits": 0, "verified_hits": 0,
                                        "baseline_events": 0, "verified_events": 0,
                                        "positive_regressions": 0, "seconds": 0.0})
        group["clips"] += 1
        group["baseline_hits"] += int(old["detected"])
        group["verified_hits"] += int(hit)
        group["baseline_events"] += len(old["events_samples"])
        group["verified_events"] += sum(e["accepted"] for e in events)
        group["positive_regressions"] += int(key[0] == "phrase" and old["detected"] and not hit)
        group["seconds"] += row["seconds"]
        traces.append({"id": sample["id"], "sha256": sample["sha256"], "events": events})
    common.write_json(output / "private-traces.json", traces)
    report = {"schema": "fono-lira-ctc-paired-replay@1", "config_sha256": digest_file(root / "config.json"),
              "source_sha256": config["source_sha256"], "cutoff": config["cutoff"], "decision_rule": rule,
              "baseline_traces_sha256_local_only": digest_file(args.baseline_traces),
              "groups": [{**g, "seconds": round(g["seconds"], 3)} for _, g in sorted(groups.items())],
              "wall_seconds": round(time.monotonic() - started, 3), "latency_measured": False,
              "idle_cpu_measured": False, "long_false_activations_measured": False, "qualified": False,
              "padding_is_negative_exposure": False, "owner_voice_used_for_training": False,
              "native_integration": False}
    common.write_json(output / "aggregate.json", report)
    public_report = {k: v for k, v in report.items() if k != "baseline_traces_sha256_local_only"}
    print(json.dumps(public_report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    calibration = sub.add_parser("calibrate")
    calibration.add_argument("--training-manifest", required=True)
    calibration.add_argument("--public-speech", required=True)
    replay = sub.add_parser("evaluate")
    replay.add_argument("--config-dir", required=True)
    replay.add_argument("--baseline-traces", required=True)
    replay.add_argument("--recordings-dir", required=True)
    exact = sub.add_parser("prepare-exact")
    exact.add_argument("--config-dir", required=True)
    exact.add_argument("--output-dir", required=True)
    for command in (calibration, replay):
        command.add_argument("--model-dir", required=True)
        command.add_argument("--archive", required=True)
        command.add_argument("--output-dir", required=True)
    arguments = parser.parse_args()
    if arguments.command == "calibrate":
        calibrate(arguments)
    elif arguments.command == "prepare-exact":
        prepare_exact(arguments)
    else:
        evaluate(arguments)
