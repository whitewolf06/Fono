import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer } from "vite";

let server;
let speechTrainer;
let speechTrainerSettings;

before(async () => {
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { hmr: false, middlewareMode: true },
  });
  speechTrainer = await server.ssrLoadModule(
    "/src/v2/features/speech-trainer/domain/speechTrainer.ts",
  );
  speechTrainerSettings = await server.ssrLoadModule(
    "/src/v2/features/speech-trainer/domain/speechTrainerSettings.ts",
  );
});

after(async () => {
  await server?.close();
});

test("period bounds include the selected calendar day", () => {
  const now = new Date("2026-08-31T15:30:00.000Z");
  const bounds = speechTrainer.getSpeechTrainerPeriodBounds(7, now);

  assert.equal(bounds.from, "2026-08-25T00:00:00.000Z");
  assert.equal(bounds.to, "2026-08-31T15:30:00.000Z");
});

test("speech findings and density remain presentational and neutral", () => {
  const report = {
    filler_count: 2,
    repetition_count: 0,
    self_correction_count: 1,
    unfinished_count: 0,
  };

  assert.equal(speechTrainer.hasSpeechFindings(report), true);
  assert.equal(speechTrainer.formatDensity(6.666), "6.7 / 100");
  assert.equal(
    speechTrainer.hasSpeechFindings({ ...report, filler_count: 0, self_correction_count: 0 }),
    false,
  );
});

test("speech trainer toggle pauses only future trainer collection", () => {
  const settings = {
    analytics_enabled: true,
    speech_trainer_enabled: true,
    history_enabled: true,
    wake_word_enabled: false,
  };

  const disabled = speechTrainerSettings.withSpeechTrainerEnabled(settings, false);

  assert.equal(disabled.analytics_enabled, true);
  assert.equal(disabled.speech_trainer_enabled, false);
  assert.equal(disabled.history_enabled, true);
  assert.equal(disabled.wake_word_enabled, false);
});
