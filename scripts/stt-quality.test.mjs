import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import {
  aggregate,
  compare,
  hash,
  modelSpec,
  validateCorpus,
} from "./stt-quality.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const directory = path.join(root, "src-tauri/tests/fixtures/stt-quality");
const manifest = JSON.parse(
  fs.readFileSync(path.join(directory, "manifest.json")),
);
test("fixed public corpus retains all audio hashes and both languages", () => {
  validateCorpus(manifest, directory);
  assert.equal(
    manifest.entries.filter((entry) => entry.condition.includes("noise"))
      .length,
    2,
  );
  assert.equal(
    manifest.entries.filter((entry) => entry.condition.includes("quiet"))
      .length,
    2,
  );
});
test("changed audio cannot silently establish a new quality baseline", () => {
  const changed = structuredClone(manifest);
  changed.entries[0].sha256 = "0".repeat(64);
  assert.throws(() => validateCorpus(changed, directory), /hash mismatch/);
});
test("WER and CER are micro-averaged from real counts, not averaged clip ratios", () => {
  const result = aggregate([
    {
      errors: {
        word_errors: 1,
        reference_words: 1,
        character_errors: 2,
        reference_characters: 2,
      },
      warm_seconds: 1,
      audio_seconds: 5,
    },
    {
      errors: {
        word_errors: 0,
        reference_words: 9,
        character_errors: 0,
        reference_characters: 18,
      },
      warm_seconds: 2,
      audio_seconds: 5,
    },
  ]);
  assert.equal(result.wer, 0.1);
  assert.equal(result.cer, 0.1);
  assert.equal(result.rtf, 0.3);
  assert.throws(() => aggregate([]), /denominator/);
});
test("the one percentage point regression gate fails beyond its threshold", () => {
  assert.equal(
    compare({ wer: 0.2, cer: 0.1 }, { wer: 0.21, cer: 0.1 }).no_wer_regression,
    false,
  );
  assert.equal(
    compare({ wer: 0.2, cer: 0.1 }, { wer: 0.21, cer: 0.1 }).passed,
    true,
  );
  assert.equal(
    compare({ wer: 0.2, cer: 0.1 }, { wer: 0.211, cer: 0.1 }).passed,
    false,
  );
});

test("public quality CI runs forks without frontend secrets and retains failure evidence", () => {
  const workflow = fs.readFileSync(
    path.join(root, ".github/workflows/stt-quality.yml"),
    "utf8",
  );
  assert.match(workflow, /\bpull_request:/);
  assert.doesNotMatch(
    workflow,
    /pull_request_target|secrets\.|NODE_AUTH_TOKEN|npm ci|needs: frontend/,
  );
  assert.match(
    workflow,
    /cargo build --locked --release --no-default-features/,
  );
  assert.match(workflow, /if: always\(\)/);
  assert.match(workflow, /path: build\/stt-quality\/report.json/);
});

test("historical baseline fixes corpus references and the CPU model identity", () => {
  const baseline = JSON.parse(
    fs.readFileSync(path.join(directory, "baseline.json")),
  );
  assert.equal(
    baseline.corpus_manifest_sha256,
    hash(Buffer.from(JSON.stringify(manifest))),
  );
  assert.equal(baseline.model.sha256, modelSpec.sha256);
  assert.ok(baseline.summary.reference_words > 300);
  assert.ok(baseline.summary.reference_characters > 1000);
});
