import assert from "node:assert/strict";
import { test } from "node:test";
import fs from "node:fs";
import {
  inputIdentity,
  summarize,
  modelFingerprint,
} from "./stt-model-comparison.mjs";
const corpus = JSON.parse(
  fs.readFileSync("src-tauri/tests/fixtures/stt-quality/manifest.json"),
);
function fixture(languages = ["ru", "en"]) {
  return {
    completed: true,
    device: "CPU",
    cold: { id: "ru-01", seconds: 1 },
    model_load_seconds: 0.5,
    preprocessing: corpus.entries.map((entry) => ({
      id: entry.id,
      source_sha256: entry.sha256,
      pcm_sha256: "a".repeat(64),
      samples: entry.samples,
      has_speech: true,
      seconds: 1,
      segments:
        entry.samples <= 400000
          ? [{ start: 0, end: entry.samples, forced: false }]
          : [
              { start: 0, end: 400000, forced: true },
              { start: 400000, end: entry.samples, forced: false },
            ],
    })),
    entries: corpus.entries.map((entry) =>
      languages.includes(entry.language)
        ? {
            id: entry.id,
            language: entry.language,
            status: "completed",
            text: entry.reference,
            warm_seconds: 1,
            audio_seconds: entry.samples / 16000,
          }
        : { id: entry.id, language: entry.language, status: "unsupported" },
    ),
  };
}
test("unsupported English is N/A and cannot dilute the Russian quality denominator", () => {
  const run = fixture(["ru"]),
    result = summarize(run, corpus, ["ru"]);
  assert.equal(result.summary.en, null);
  assert.equal(result.summary.ru.utterances, 10);
  assert.equal(result.entries.length, 10);
  run.entries[0] = { ...fixture().entries[0] };
  assert.throws(() => summarize(run, corpus, ["ru"]));
});
test("an empty admitted hypothesis stays in the quality score", () => {
  const run = fixture(),
    index = run.entries.findIndex((entry) => entry.language === "ru");
  run.entries[index].text = "";
  run.entries[index].warm_seconds = 0;
  run.preprocessing[index].has_speech = false;
  const result = summarize(run, corpus, ["ru", "en"]);
  assert.equal(result.summary.ru.utterances, 10);
  assert.ok(result.summary.ru.word_errors > 0);
});
test("incomplete runs, changed inputs and mismatched durations cannot become a table", () => {
  for (const change of [
    (run) => {
      run.completed = false;
    },
    (run) => {
      run.entries.pop();
    },
    (run) => {
      run.entries[0].audio_seconds = 1;
    },
    (run) => {
      run.entries[0].warm_seconds = NaN;
    },
    (run) => {
      run.preprocessing[0].source_sha256 = "b".repeat(64);
    },
    (run) => {
      run.preprocessing[0].segments[0].start = 1;
    },
    (run) => {
      run.cold.id = "en-01";
    },
  ]) {
    const run = fixture();
    change(run);
    assert.throws(() => summarize(run, corpus, ["ru", "en"]));
  }
});
test("PCM identity ignores measurement time but detects an input or speech decision change", () => {
  const first = fixture(),
    second = fixture();
  second.preprocessing[0].seconds = 5;
  assert.equal(inputIdentity(first, corpus), inputIdentity(second, corpus));
  second.preprocessing[0].pcm_sha256 = "b".repeat(64);
  assert.notEqual(inputIdentity(first, corpus), inputIdentity(second, corpus));
  second.preprocessing[0].pcm_sha256 = "a".repeat(64);
  second.preprocessing[0].has_speech = false;
  assert.notEqual(inputIdentity(first, corpus), inputIdentity(second, corpus));
});
test("measurement metadata is matched to exact model artifacts independently of file order", () => {
  const files = [
    { filename: "encoder.onnx", bytes: 10, sha256: "a".repeat(64) },
    { filename: "tokens.txt", bytes: 2, sha256: "b".repeat(64) },
  ];
  assert.equal(modelFingerprint(files), modelFingerprint([...files].reverse()));
  assert.notEqual(
    modelFingerprint(files),
    modelFingerprint([{ ...files[0], sha256: "c".repeat(64) }, files[1]]),
  );
});
