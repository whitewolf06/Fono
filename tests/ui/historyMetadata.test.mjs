import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";

let server,
  historyMetadataFromNative,
  dictationFacts,
  dictationStages,
  durationLabel,
  exactDateLabel;
before(async () => {
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { hmr: false, middlewareMode: true },
  });
  ({ historyMetadataFromNative } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/historyMetadata.ts",
  ));
  ({ dictationFacts, dictationStages, durationLabel, exactDateLabel } =
    await server.ssrLoadModule(
      "/src/v3/features/history/application/statistics.ts",
    ));
});
after(async () => await server?.close());

test("stored backend is actual CPU even when CUDA was requested; duration is independent of analytics", () => {
  const metadata = historyMetadataFromNative({
    device: "CPU",
    processing: null,
    metadata: {
      recording_duration_ms: 12500,
      generation_duration_ms: 2100,
      recognition_duration_ms: 1500,
      model_load_duration_ms: 200,
      processing_duration_ms: 300,
      backend: "cpu",
      model: "ggml-large-v3-turbo.bin",
      requested_acceleration: "cuda",
      language: "ru",
      detected_language: "ru",
      dictation_mode: "standard",
      processing_mode: "clean",
      dictionary_enabled: true,
    },
  });
  const facts = dictationFacts({ duration: 0, metadata });
  assert.deepEqual(
    facts.find((f) => f.title === "Фактический backend"),
    { title: "Фактический backend", value: "CPU" },
  );
  assert.deepEqual(
    facts.find((f) => f.title === "Выбранное ускорение"),
    { title: "Выбранное ускорение", value: "NVIDIA CUDA" },
  );
  assert.match(facts.find((f) => f.title === "Запись").value, /12,5 с/);
  assert.match(
    facts.find((f) => f.title === "Генерация текста").value,
    /2,1 с/,
  );
  assert.deepEqual(dictationStages({ duration: 0, metadata }), [
    { title: "Подготовка модели", value: "200 мс" },
    { title: "Обработка текста", value: "300 мс" },
    { title: "Определённый язык", value: "Русский" },
  ]);
});

test("old records preserve available backend and do not invent model or generation", () => {
  const metadata = historyMetadataFromNative({
    device: "Vulkan",
    processing: { audio_secs: 2.5, transcribe_secs: 0.8, ai_mode: "off" },
  });
  assert.equal(metadata.legacy, true);
  assert.equal(metadata.backend, "vulkan");
  assert.equal(metadata.recordingDurationMs, 2500);
  assert.equal(metadata.recognitionDurationMs, 800);
  assert.equal(metadata.model, undefined);
  assert.equal(metadata.generationDurationMs, undefined);
  assert.deepEqual(dictationStages({ duration: 2.5, metadata }), []);
  const facts = dictationFacts({ duration: 2.5, metadata });
  assert.equal(
    facts.find((f) => f.title === "Генерация текста").value,
    "Не сохранено",
  );
  assert.equal(
    historyMetadataFromNative({ device: null, processing: null }),
    undefined,
  );
});

test("unknown durations are explicit; zero and exact date seconds are preserved", () => {
  assert.equal(durationLabel(null), "Не сохранено");
  assert.equal(durationLabel(NaN), "Не сохранено");
  assert.equal(durationLabel(0), "0 мс");
  assert.match(exactDateLabel("2026-10-04T10:15:37Z"), /04\.10\.2026/);
  assert.match(exactDateLabel("2026-10-04T10:15:37Z"), /:37/);
  assert.deepEqual(
    dictationStages({
      metadata: { modelLoadDurationMs: 0, processingDurationMs: NaN },
    }),
    [{ title: "Подготовка модели", value: "0 мс" }],
  );
});
