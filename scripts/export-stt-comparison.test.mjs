import assert from "node:assert/strict";
import { test } from "node:test";
import { exportData } from "./export-stt-comparison.mjs";
test("only completed measurements with common PCM can be exported to UI", () => {
  const source = {
    schema: 1,
    task: "FONO-119",
    same_preprocessing: true,
    completed: false,
    completed_at: "2026-10-19T12:00:00Z",
    hardware: { cpu: "test CPU" },
    corpus: { name: "fixed" },
    runs: [
      {
        status: "incomplete",
        id: { engine: "whisper", variant: "large_turbo" },
      },
      {
        status: "completed",
        id: { engine: "giga_am", variant: "v3_e2e_rnnt_int8" },
        fingerprint: "verified",
        raw: { profile: "debug", model_load_seconds: 1, cold: { seconds: 2 } },
        summary: { ru: { wer: 0.1, cer: 0.02, rtf: 0.2 }, en: null },
      },
    ],
  };
  const data = exportData(source);
  assert.equal(data.models.length, 1);
  assert.equal(data.complete, false);
  assert.equal(data.models[0].id, "giga_am:v3_e2e_rnnt_int8");
  assert.equal(data.models[0].en, null);
  assert.throws(() => exportData({ ...source, same_preprocessing: false }));
  assert.throws(() => exportData({ ...source, runs: [] }));
});
