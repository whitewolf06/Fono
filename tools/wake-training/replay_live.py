"""One paired local regression: pinned v0.7.73 vs current live worker.

Windows native VAD, existing consented WAVs only. No microphone, training,
network, parameter search or private audio/transcripts in the output.
Use --isolated for the compatibility set; default adds ordinary-speech context.
Prepared build/wake-* assets and local native dependency cache are required.
"""
import ctypes as c
import hashlib
import os
import json
from pathlib import Path
import subprocess
import sys
import time
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/wake-training"))
import candidate as common
import dev_check
from live_worker import Stream
import live_runtime
runtime_parity = "--runtime-parity" in sys.argv
prefill_parity = "--prefill-parity" in sys.argv
os.environ["OPENBLAS_NUM_THREADS"] = "1"
os.environ["OMP_NUM_THREADS"] = "1"
manifest = json.loads((ROOT / "scripts/ci-native-dependencies.json").read_text())
lib = ROOT / "build/ci-dependencies" / ("sherpa-verified-" + manifest["sherpa"]["sha256"]) / manifest["sherpa"]["directory"] / "lib"
dll_directory = os.add_dll_directory(str(lib))
dll = c.CDLL(str(lib / "sherpa-onnx-c-api.dll"))

class Model(c.Structure):
    _fields_ = [("model", c.c_char_p), ("threshold", c.c_float), ("min_silence_duration", c.c_float),
                ("min_speech_duration", c.c_float), ("window_size", c.c_int32), ("max_speech_duration", c.c_float)]
class Config(c.Structure):
    _fields_ = [("silero_vad", Model), ("sample_rate", c.c_int32), ("num_threads", c.c_int32),
                ("provider", c.c_char_p), ("debug", c.c_int32), ("ten_vad", Model)]
class Segment(c.Structure):
    _fields_ = [("start", c.c_int32), ("samples", c.POINTER(c.c_float)), ("n", c.c_int32)]
def api(name, arguments, result=None):
    function = getattr(dll, "SherpaOnnx" + name)
    function.argtypes, function.restype = arguments, result
    return function
create = api("CreateVoiceActivityDetector", [c.POINTER(Config), c.c_float], c.c_void_p)
destroy = api("DestroyVoiceActivityDetector", [c.c_void_p])
accept = api("VoiceActivityDetectorAcceptWaveform", [c.c_void_p, c.POINTER(c.c_float), c.c_int32])
detected = api("VoiceActivityDetectorDetected", [c.c_void_p], c.c_int32)
empty = api("VoiceActivityDetectorEmpty", [c.c_void_p], c.c_int32)
front = api("VoiceActivityDetectorFront", [c.c_void_p], c.POINTER(Segment))
free = api("DestroySpeechSegment", [c.POINTER(Segment)])
pop = api("VoiceActivityDetectorPop", [c.c_void_p])
reset = api("VoiceActivityDetectorReset", [c.c_void_p])
baseline_ref = ("f46308c8c79f086e17512f7877085a6d7474abf4" if prefill_parity else
                "47cef4966de9d18a9375491e78cd8f3f7b5757d2" if runtime_parity else "a26597ce61215ce14d0015f96ab3124842442cf6")
baseline_source = subprocess.run(["git", "show", baseline_ref + ":tools/wake-training/live_worker.py"], cwd=ROOT,
                                 check=True, capture_output=True).stdout
baseline = {"__name__": "fixed_baseline"}
exec(compile(baseline_source, "fixed_baseline", "exec"), baseline)
frozen_runtime = None
if prefill_parity:
    frozen_source = subprocess.run(["git", "show", baseline_ref + ":tools/wake-training/live_runtime.py"],
                                  cwd=ROOT, check=True, capture_output=True).stdout
    frozen_runtime = {"__name__": "fixed_runtime"}
    exec(compile(frozen_source, "fixed_runtime", "exec"), frozen_runtime)
np, cnn, tone = dev_check.prepare(inference_only=prefill_parity)
traces = json.loads((ROOT / "build/wake-lira-candidate/ctc-exact-owner-replay-02/private-traces.json").read_text())
records = []
for trace in traces:
    folder = ROOT / "build/dev-desktop-profile/wake-recordings" / trace["id"]
    meta = json.loads((folder / "recording.json").read_text(encoding="utf-8"))
    assert meta["permission"] == "consented-local-only" and meta["split"] == "dev"
    assert common.digest(folder / "recording.wav") == trace["sha256"]
    records.append((trace, meta["sample"], common.read_pcm(folder / "recording.wav")))
room = next(pcm for _, sample, pcm in records if sample["kind"] == "background" and np.any(pcm))
context = next(pcm for t, sample, pcm in records if sample["kind"] == "other_speech" and t["events"] and not any(e["accepted"] for e in t["events"]))
def background(n): return np.tile(room, n // len(room) + 1)[:n]
def gated(pcm):
    reset(vad)
    pending = np.zeros(0, dtype=np.float32)
    processed, last_speech, flags = 0, None, []
    for frame in pcm.reshape(-1, common.STRIDE):
        pending = np.concatenate((pending, frame))
        while len(pending) >= 512:
            window, pending = pending[:512].copy(), pending[512:]
            accept(vad, window.ctypes.data_as(c.POINTER(c.c_float)), 512)
            processed += 512
            if detected(vad): last_speech = processed
            while not empty(vad):
                segment = front(vad)
                last_speech = max(last_speech or 0, segment.contents.start + segment.contents.n)
                free(segment)
                pop(vad)
        flags.append(last_speech is not None and processed - last_speech <= common.WINDOW)
    return flags
vad_path = str(ROOT / "src-tauri/resources/vad/silero_vad.onnx").encode()
vad = create(c.byref(Config(Model(vad_path, .5, .35, .12, 512, 30), 16000, 1, b"cpu", 0, Model())), 30)
assert vad
rows = []
isolated = "--isolated" in sys.argv
started = time.monotonic()
try:
    cases = []
    for trace, sample, pcm in records:
        positive = sample["kind"] == "phrase"
        prefix = np.zeros(0, dtype=np.float32) if isolated else (np.concatenate((context, background(4800))) if positive else background(80000))
        end = len(prefix) + len(pcm)
        cases.append((positive, sample["condition"], np.concatenate((prefix, pcm, background(48000))), len(prefix), end))
    public = common.read_pcm(ROOT / "src-tauri/tests/fixtures/stt-quality/ru-01.wav")
    cases.append((False, "public", np.concatenate((background(80000), public, background(48000))), 80000, 80000+len(public)))
    if isolated:
        for folder in sorted((ROOT/'build/dev-desktop-profile/wake-recordings').iterdir()):
            if not folder.is_dir() or folder.is_symlink() or folder.is_junction(): continue
            meta = json.loads((folder/'recording.json').read_text(encoding='utf-8'))
            sample = meta['sample']
            if sample.get('phraseId') == 'privet_lira' or sample['kind'] != 'other_speech': continue
            assert meta['permission'] == 'consented-local-only' and meta['split'] == 'dev'
            assert common.digest(folder/'recording.wav') == sample['sha256']
            legacy = common.read_pcm(folder/'recording.wav')
            cases.append((False,'legacy_other_speech',np.concatenate((legacy,background(48000))),0,len(legacy)))
    for number, (positive, condition, pcm, onset, end) in enumerate(cases):
        pcm = np.pad(pcm, (0, (-len(pcm)) % common.STRIDE))
        flags = gated(pcm)
        padded = np.pad(pcm, (common.WINDOW, 0))
        scores, focused_scores, runtime_scores, runtime_focused_scores = [], [], [], []
        for offset in range(0, len(flags), common.BATCH):
            windows = np.stack([padded[(i+1)*common.STRIDE:(i+1)*common.STRIDE+common.WINDOW]
                                for i in range(offset, min(offset+common.BATCH, len(flags)))])
            features = live_runtime.frontend(windows) if prefill_parity else common.frontend(windows).numpy()
            scores.extend(cnn.run(None, {"logmel":features})[0].ravel().tolist())
            focused = windows.copy()
            focused[:, :common.WINDOW//2] = 0
            features = live_runtime.frontend(focused) if prefill_parity else common.frontend(focused).numpy()
            focused_scores.extend(cnn.run(None, {"logmel":features})[0].ravel().tolist())
            if runtime_parity:
                runtime_scores.extend(cnn.run(None, {"logmel":live_runtime.frontend(windows)})[0].ravel().tolist())
                runtime_focused_scores.extend(cnn.run(None, {"logmel":live_runtime.frontend(focused)})[0].ravel().tolist())
        if runtime_parity:
            np.testing.assert_allclose(runtime_scores, scores, rtol=1e-4, atol=1e-5)
            np.testing.assert_allclose(runtime_focused_scores, focused_scores, rtol=1e-4, atol=1e-5)
        cache, result = {}, {}
        for label, cls in (("baseline", baseline["Stream"]), ("candidate", Stream)):
            checks, events = [], []
            position = 0
            def classify(_):
                if prefill_parity:
                    return max(scores[position], focused_scores[position])
                if runtime_parity:
                    return (max(scores[position], focused_scores[position]) if label == "baseline"
                            else max(runtime_scores[position], runtime_focused_scores[position]))
                return scores[position] if label == "baseline" else max(scores[position], focused_scores[position])
            def verify(window):
                key = (label if runtime_parity else "shared", hashlib.sha256(window.tobytes()).digest())
                if key not in cache:
                    cache[key] = (frozen_runtime["verify"](tone, window) if prefill_parity
                                  else live_runtime.verify(tone, window) if runtime_parity and label == "candidate"
                                  else tone.score(window)["greedy_whole_phrase"])
                value = cache[key]
                checks.append(value)
                return value
            stream = (cls(classify, verify, prefill=lambda: live_runtime.IncrementalVerifier(tone))
                      if prefill_parity and label == "candidate" else cls(classify, verify))
            worker_ready_ms, ready_events, max_frame_ms = 0.0, [], 0.0
            for position, frame in enumerate(pcm.reshape(-1, common.STRIDE)):
                sent = time.perf_counter()
                accepted = stream.accept(frame, flags[position])["detected"]
                frame_ms = (time.perf_counter() - sent) * 1000
                max_frame_ms = max(max_frame_ms, frame_ms)
                worker_ready_ms = max(worker_ready_ms, (position+1)*80) + frame_ms
                if accepted:
                    events.append((position+1)*common.STRIDE)
                    ready_events.append(worker_ready_ms)
            result[label] = {"events":len(events), "detected":any(e>onset for e in events),
                             "early_events":sum(e<=onset for e in events), "checks":stream.stats["checks"],
                             "first_ready_after_recording_end_ms": min(((e-end)/16 for e in events if e>onset), default=None)}
            if prefill_parity:
                result[label].update(event_samples=events, max_frame_ms=round(max_frame_ms,3),
                    simulated_worker_ready_ms=[round(t,3) for t in ready_events])
        row = {"case":number, "positive":positive, "condition":condition,
               "max_full_score":max(scores), "max_focused_score":max(focused_scores), **result}
        rows.append(row)
    report = {"schema":"fono-lira-paired-context-regression@1", "models_and_threshold_unchanged":True,
              "live_policy_sha256":common.digest(ROOT/'tools/wake-training/lira-live-policy.json'),
              "candidate_context_samples":[64000,32000],
              "native_vad":True, "microphone_opened":False, "owner_recordings_used_for_training":False,
              "baseline_source_sha256":hashlib.sha256(baseline_source).hexdigest(),
              "candidate_source_sha256":common.digest(ROOT/'tools/wake-training/live_worker.py'),
              "runtime_parity": runtime_parity,
              "prefill_parity": prefill_parity,
              "runtime_source_sha256": common.digest(ROOT/'tools/wake-training/live_runtime.py') if runtime_parity else None,
              "context":("isolated existing clips; legacy speech negatives included; room continuation looped" if isolated else "same existing hard negative before each positive; recorded room looped, no independent exposure"),
              "latency":"sample clock relative to recording file end only; not labelled phrase end or live latency/p95",
              "wall_seconds":round(time.monotonic()-started,3), "rows":rows}
    for label in ("baseline", "candidate"):
        report[label] = {"positive_clips":sum(r['positive'] for r in rows),
                         "positive_hits":sum(r['positive'] and r[label]['detected'] for r in rows),
                         "negative_clips":sum(not r['positive'] for r in rows),
                         "negative_events":sum(r[label]['events'] for r in rows if not r['positive']),
                         "early_events_in_positive_context":sum(r[label]['early_events'] for r in rows if r['positive']),
                         "ctc_checks":sum(r[label]['checks'] for r in rows)}
    report['lost_positive_cases'] = [r['case'] for r in rows if r['positive'] and r['baseline']['detected'] and not r['candidate']['detected']]
    if prefill_parity:
        keys = ("events", "detected", "early_events", "checks", "first_ready_after_recording_end_ms", "event_samples")
        report["identical_decisions_counts_and_sample_clocks"] = all(
            all(row["baseline"][key] == row["candidate"][key] for key in keys) for row in rows)
        report["timing_limit"] = "Serial 80ms worker simulation using measured per-frame work, with cached identical CNN scores; excludes real microphone, CNN work, pipes, cue output and UI. Not live latency or p95."
    output=ROOT/('build/checks/lira-prefill-parity.json' if prefill_parity else 'build/checks/lira-runtime-parity.json' if runtime_parity else ('build/checks/lira-paired-isolated.json' if isolated else 'build/checks/lira-paired-live-retry.json'))
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps({k:v for k,v in report.items() if k!='rows'}),flush=True)
    assert not report['lost_positive_cases']
    assert report['candidate']['negative_events'] <= report['baseline']['negative_events']
    assert report['candidate']['early_events_in_positive_context'] <= report['baseline']['early_events_in_positive_context']
    if prefill_parity:
        assert report["identical_decisions_counts_and_sample_clocks"]
    if runtime_parity:
        assert all(row['baseline'] == row['candidate'] for row in rows), "runtime changed a fixed-case decision or sample clock"
finally:
    destroy(vad)
