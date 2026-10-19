import assert from "node:assert/strict";
import { test } from "node:test";
import fs from "node:fs";
import { verifyRun, comparisons } from "./stt-quantized-quality.mjs";
import { score } from "./stage5-quality.mjs";

const corpus = JSON.parse(
  fs.readFileSync("src-tauri/tests/fixtures/stt-quality/manifest.json"),
);
function fixture() {
  return {
    backend: "cpu",
    threads: 8,
    decoding: { strategy: "fast" },
    entries: corpus.entries.map((entry) => ({
      id: entry.id,
      language: entry.language,
      text: entry.reference,
      audio_seconds: entry.samples / entry.sample_rate,
      warm_seconds: 1,
      errors: score(entry.reference, entry.reference),
    })),
  };
}
test("quantized qualification rejects changed corpus, scorer, policy and timing", () => {
  assert.equal(verifyRun(fixture(), corpus).all.wer, 0);
  for (const change of [
    (run) => {
      run.entries[0].id = run.entries[1].id;
    },
    (run) => {
      run.entries[0].language = "ru";
    },
    (run) => {
      run.entries[0].errors.word_errors = 1;
    },
    (run) => {
      run.entries[0].audio_seconds += 1;
    },
    (run) => {
      run.entries[0].warm_seconds = -1;
    },
    (run) => {
      run.entries.pop();
    },
    (run) => {
      run.threads = 4;
    },
    (run) => {
      run.decoding.strategy = "accurate";
    },
  ]) {
    const run = fixture();
    change(run);
    assert.throws(() => verifyRun(run, corpus));
  }
});
test("quality regression is recorded per language with the unchanged CI-03 one-point limit", () => {
  const baseline = Object.fromEntries(
    ["all", "ru", "en"].map((key) => [key, { wer: 0.1, cer: 0.04 }]),
  );
  const result = comparisons(baseline, {
    ...baseline,
    ru: { wer: 0.12, cer: 0.05 },
  });
  assert.equal(result.ru.passed, false);
  assert.equal(result.ru.maximum_wer_regression, 0.01);
  assert.equal(result.en.passed, true);
});
