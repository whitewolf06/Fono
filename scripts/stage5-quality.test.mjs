import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import {
  compareLatency,
  consolidate,
  percentile,
  score,
} from "./stage5-quality.mjs";

const read = (name) =>
  JSON.parse(fs.readFileSync(new URL(name, import.meta.url)));
const manifest = read("../src-tauri/tests/fixtures/stt-quality/manifest.json");
const frozen = read("../src-tauri/tests/fixtures/stt-quality/baseline.json");
// Historical reports are test fixtures only. Production consolidation rejects them
// because they lack the new common-run lineage; these copies exercise mutations.
function fixture(runId = "unit-test-candidate-run") {
  const resampling = read(
    "../docs/archive/improvement-plan/evidence/stage2-stt-quality-final.json",
  );
  const vad = read(
    "../docs/archive/improvement-plan/evidence/stage4-quality.json",
  );
  const lineage = {
    run_id: runId,
    source_revision: "1".repeat(40),
    source_inputs_sha256: "2".repeat(64),
    app_version: "0.0.1",
  };
  resampling.lineage = structuredClone(lineage);
  vad.lineage = structuredClone(lineage);
  vad.profile = resampling.profile;
  resampling.runner_binary_sha256 = vad.binaries.runner_sha256;
  for (const variant of Object.values(resampling.alternatives))
    variant.decoding = { strategy: "fast", threads: null };
  return { resampling, vad };
}
const run = ({ resampling, vad }, options) =>
  consolidate(resampling, vad, manifest, frozen, options);

test("one identified replay retains numeric gates and open PERF-04 criterion separately", () => {
  const result = run(fixture());
  assert.equal(result.passed, true);
  assert.equal(
    result.quality_gates.ci03_same_run_wer_plus_one_percentage_point.passed,
    true,
  );
  assert.equal(
    result.open_acceptance.perf04_no_wer_regression.state,
    "failed_open",
  );
  assert.equal(result.open_acceptance.perf04_no_wer_regression.passed, false);
  assert.equal(result.latency_acceptance.passed, null);
  for (const variant of Object.values(result.variants)) {
    assert.equal(variant.summary.reference_words, 450);
    assert.equal(variant.summary.reference_characters, 2846);
    assert.equal(variant.strata.quiet.utterances, 2);
    assert.equal(variant.strata.noise.utterances, 2);
    assert.ok(variant.strata.short_under_10_seconds.utterances > 0);
    assert.ok(variant.strata.long_at_least_10_seconds.utterances > 0);
    assert.equal(variant.latency.warm_utterances, 20);
    assert.ok(variant.latency.warm_seconds.p95 > 0);
  }
});

test("historical evidence without lineage cannot be passed off as one current run", () => {
  const value = fixture();
  delete value.resampling.lineage;
  assert.throws(() => run(value), /lineage/);
  for (const field of [
    "run_id",
    "source_revision",
    "source_inputs_sha256",
    "app_version",
  ]) {
    const changed = fixture();
    changed.vad.lineage[field] =
      field === "run_id"
        ? "different-run"
        : field === "app_version"
          ? "0.0.2"
          : "3".repeat(field === "source_revision" ? 40 : 64);
    assert.throws(() => run(changed), /lineage/);
  }
});

test("changed model, corpus, binary, hardware, profile and decoding fail closed", () => {
  for (const mutate of [
    (value) => {
      value.vad.model.sha256 = "a".repeat(64);
    },
    (value) => {
      value.vad.corpus_manifest_sha256 = "a".repeat(64);
    },
    (value) => {
      value.vad.binaries.runner_sha256 = "a".repeat(64);
    },
    (value) => {
      value.vad.hardware.cpu = "different cpu";
    },
    (value) => {
      value.vad.profile = "different profile";
    },
    (value) => {
      value.vad.variants.neural_fast.decoding.strategy = "accurate";
    },
    (value) => {
      value.vad.variants.neural_fast.threads = 3;
    },
    (value) => {
      value.resampling.alternatives.legacy.replay_hashes[0].sha256 = "a".repeat(
        64,
      );
    },
  ]) {
    const value = fixture();
    mutate(value);
    assert.throws(() => run(value), /mismatch|Invalid/);
  }
});

test("missing, duplicate, reordered and silently shortened clips fail coverage validation", () => {
  for (const mutate of [
    (entries) => entries.pop(),
    (entries) => {
      entries[1] = entries[0];
    },
    (entries) => entries.reverse(),
    (entries) => {
      entries[0].audio_seconds /= 2;
      entries[0].rtf *= 2;
    },
    (entries) => {
      entries[0].errors.reference_words--;
    },
  ]) {
    const value = fixture();
    mutate(value.vad.variants.neural_fast.entries);
    assert.throws(() => run(value), /mismatch/);
  }
});

test("negative, infinite, NaN, missing and inconsistent latency or quality values are rejected", () => {
  for (const invalid of [-1, Infinity, NaN, undefined]) {
    const value = fixture();
    value.vad.variants.neural_fast.entries[0].warm_seconds = invalid;
    assert.throws(() => run(value), /Invalid measurement/);
  }
  for (const mutate of [
    (variant) => {
      delete variant.cold;
    },
    (variant) => {
      variant.model_load_seconds = -1;
    },
    (variant) => {
      variant.entries[0].rtf += 1;
    },
    (variant) => {
      variant.entries[0].errors.wer += 1;
    },
    (variant) => {
      variant.entries[0].errors.character_errors = 0.5;
    },
    (variant) => {
      variant.entries[0].no_speech = true;
    },
  ]) {
    const value = fixture();
    mutate(value.vad.variants.neural_fast);
    assert.throws(
      () => run(value),
      /Missing cold|Invalid|Inconsistent|hid errors|mismatch/,
    );
  }
});

test("VAD false negatives keep all reference words and fail the unchanged strict gate", () => {
  const value = fixture();
  value.vad.boundaries[1].has_speech = false;
  for (const strategy of ["fast", "accurate"]) {
    const clip = value.vad.variants[`neural_${strategy}`].entries[1];
    clip.no_speech = true;
    clip.text = "";
    clip.warm_seconds = 0;
    clip.rtf = 0;
    clip.errors.word_errors = clip.errors.reference_words;
    clip.errors.character_errors = clip.errors.reference_characters;
    clip.errors.wer = 1;
    clip.errors.cer = 1;
  }
  const result = run(value);
  assert.equal(result.passed, false);
  assert.equal(result.variants.neural_fast.summary.reference_words, 450);
  assert.equal(result.variants.neural_fast.latency.speech_gate_rejections, 1);
  assert.equal(
    result.quality_gates.arc09_production_fast_no_wer_regression.passed,
    false,
  );
});

test("CI-03 one-point regression threshold is still enforced", () => {
  const value = fixture();
  const clip = value.resampling.alternatives.current.entries[0];
  clip.text = "wrong ".repeat(20) + clip.text;
  clip.errors = score(manifest.entries[0].reference, clip.text);
  const result = run(value);
  assert.equal(result.passed, false);
  assert.equal(
    result.quality_gates.ci03_same_run_wer_plus_one_percentage_point.passed,
    false,
  );
});

test("explicit same-hardware latency baseline fails regressions without conflating quality", () => {
  const baseline = run(fixture("unit-test-baseline-run"));
  const candidate = run(fixture("unit-test-candidate-run"));
  candidate.variants.neural_fast.latency.per_clip[0].warm_seconds *= 2;
  assert.equal(
    compareLatency(baseline, run(fixture()), 1.2, 0.01).passed,
    true,
  );
  assert.equal(compareLatency(baseline, candidate, 1.2, 0.01).passed, false);
  const raw = fixture();
  raw.vad.variants.neural_fast.entries[0].warm_seconds *= 2;
  raw.vad.variants.neural_fast.entries[0].rtf *= 2;
  const integrated = run(raw, {
    latencyBaseline: baseline,
    maximumRatio: 1.2,
    slackSeconds: 0.01,
  });
  assert.equal(integrated.passed, false);
  assert.equal(
    integrated.quality_gates.arc09_production_fast_no_wer_regression.passed,
    true,
  );
});

test("latency baseline refuses changed hardware/options/coverage/method and invalid explicit bounds", () => {
  const candidate = run(fixture());
  for (const mutate of [
    (before) => {
      before.hardware.cpu = "other machine";
    },
    (before) => {
      before.profile = "release";
    },
    (before) => {
      before.variants.neural_fast.threads = 2;
    },
    (before) => {
      before.variants.neural_fast.latency.per_clip.pop();
    },
    (before) => {
      before.variants.neural_fast.latency.per_clip[0].warm_seconds = NaN;
    },
    (before) => {
      before.variants.neural_fast.latency.method = "different method";
    },
    (before) => {
      before.variants.neural_fast.input_hashes[0].sha256 = "a".repeat(64);
    },
  ]) {
    const before = structuredClone(candidate);
    before.lineage.run_id = "unit-test-independent-baseline-run";
    mutate(before);
    assert.throws(
      () => compareLatency(before, candidate, 1.2, 0.01),
      /mismatch|Missing|Invalid/,
    );
  }
  for (const [ratio, slack] of [
    [NaN, 0],
    [0.5, 0],
    [1, -1],
    [1, undefined],
  ])
    assert.throws(
      () => compareLatency(candidate, candidate, ratio, slack),
      /Invalid|at least 1/,
    );
});

test("a latency baseline needs valid lineage and a different run, never itself", () => {
  const candidate = run(fixture());
  assert.throws(
    () => compareLatency(candidate, candidate, 1.2, 0.01),
    /independent run_id/,
  );
  for (const mutate of [
    (before) => {
      delete before.lineage;
    },
    (before) => {
      before.lineage.run_id = " ";
    },
    (before) => {
      before.lineage.source_revision = "unknown";
    },
    (before) => {
      before.lineage.source_inputs_sha256 = "unknown";
    },
    (before) => {
      before.lineage.app_version = "unknown";
    },
  ]) {
    const before = structuredClone(candidate);
    before.lineage.run_id = "unit-test-independent-baseline-run";
    mutate(before);
    assert.throws(
      () => compareLatency(before, candidate, 1.2, 0.01),
      /lineage/,
    );
  }
});

test("changed or empty hypotheses cannot keep stale zero/old error counts", () => {
  for (const hypothesis of [
    "",
    "подменённая гипотеза",
    "totally changed recognized text",
  ]) {
    const value = fixture();
    value.vad.variants.original_fast.entries[0].text = hypothesis;
    assert.throws(() => run(value), /hypothesis error counts/);
  }
});

test("independent scorer preserves Russian repetition, punctuation, yo and Unicode scalar counts without NFKC", () => {
  assert.deepEqual(score("Да, да!", "да"), {
    word_errors: 1,
    reference_words: 2,
    character_errors: 3,
    reference_characters: 5,
    wer: 0.5,
    cer: 0.6,
  });
  assert.deepEqual(score("Привет, ёж! Ёж?", "ПРИВЕТ ёж"), {
    word_errors: 1,
    reference_words: 3,
    character_errors: 3,
    reference_characters: 12,
    wer: 1 / 3,
    cer: 0.25,
  });
  assert.equal(score("ё", "е").character_errors, 1);
  assert.equal(score("𐐀", "").reference_characters, 1);
  assert.equal(score("𐐀", "𐐨").character_errors, 0);
  assert.equal(score("①", "1").character_errors, 1);
  assert.equal(score("two words", "").word_errors, 2);
});

test("nearest-rank quantiles are explicit and never fabricate an empty timing population", () => {
  assert.equal(percentile([4, 1, 3, 2], 0.5), 2);
  assert.equal(percentile([4, 1, 3, 2], 0.95), 4);
  assert.throws(() => percentile([], 0.5), /Empty/);
  assert.throws(() => percentile([NaN], 0.5), /Invalid/);
  assert.throws(() => percentile([1], 0), /Invalid/);
});

test("quality workflow identifies one run, triggers on runner inputs and retains consolidation after a failed predecessor", () => {
  const workflow = fs.readFileSync(
    new URL("../.github/workflows/stt-quality.yml", import.meta.url),
    "utf8",
  );
  assert.match(
    workflow,
    /STT_QUALITY_RUN_ID: \$\{\{ github\.run_id \}\}-\$\{\{ github\.run_attempt \}\}/,
  );
  for (const input of [
    "src-tauri/crates/fono-stt-worker/**",
    "src-tauri/crates/fono-stt-protocol/**",
    "src-tauri/Cargo.lock",
    "scripts/stt-quality*",
    "scripts/stage4-quality*",
    "scripts/stage5-quality*",
  ])
    assert.ok(
      workflow.includes(`"${input}"`),
      `missing quality input trigger: ${input}`,
    );
  assert.match(
    workflow,
    /Consolidate same-run quality, cold and warm latency evidence\r?\n\s+if: \$\{\{ !cancelled\(\) \}\}/,
  );
  assert.match(
    workflow,
    /run: node scripts\/stage5-quality\.mjs --report build\/stt-quality\/stage5-report\.json/,
  );
  assert.match(
    workflow,
    /Save numerical quality evidence\r?\n\s+if: always\(\)/,
  );
  assert.match(workflow, /build\/stt-quality\/stage5-report\.json/);
  assert.doesNotMatch(
    workflow,
    /secrets\.|pull_request_target|NODE_AUTH_TOKEN/,
  );
});
