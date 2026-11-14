import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";
import { createSSRApp, h } from "vue";
import { renderToString } from "vue/server-renderer";

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

test("fallback is explicit in history and is never inferred for older entries", () => {
  const metadata = historyMetadataFromNative({
    device: "CPU",
    metadata: {
      fallback: true,
      backend: "cpu",
      model: "gigaam-v3",
      requested_acceleration: "cpu",
    },
  });
  assert.equal(metadata.fallback, true);
  assert.deepEqual(
    dictationFacts({ duration: 1, metadata }).find(
      (f) => f.title === "Источник распознавания",
    ),
    { title: "Источник распознавания", value: "Резерв · локально на CPU" },
  );
  assert.equal(
    dictationFacts({ duration: 1, metadata: { backend: "cpu" } }).some(
      (f) => f.title === "Источник распознавания",
    ),
    false,
  );
});

test("remote history shows per-session server facts and measured stages independently of local preferences", () => {
  const metadata = historyMetadataFromNative({
    device: "remote:cuda:0",
    metadata: {
      backend: "remote",
      model: "whisper:large_turbo",
      remote_name: "Kari",
      remote_device: "cuda:0",
      remote_backend: "cuda",
      requested_acceleration: "vulkan",
      recognition_duration_ms: 999,
      model_load_duration_ms: 30,
      request_duration_ms: 420,
      server_total_duration_ms: 380,
      decode_duration_ms: 220,
      network_duration_ms: 0,
      remote_url: "https://PRIVATE_SERVER",
      token: "PRIVATE_TOKEN",
      ca_pem: "PRIVATE_CA",
    },
  });
  assert.equal(metadata.backend, "remote");
  assert.equal(metadata.remoteName, "Kari");
  assert.equal(metadata.remoteDevice, "cuda:0");
  assert.equal(metadata.remoteBackend, "cuda");
  assert.equal(metadata.recognitionDurationMs, 220);
  assert.equal(metadata.networkDurationMs, 0);
  assert.ok(!JSON.stringify(metadata).includes("PRIVATE"));
  const facts = dictationFacts({ duration: 5, metadata });
  assert.deepEqual(
    facts.find((f) => f.title === "Фактический backend"),
    { title: "Фактический backend", value: "Сервер Fono" },
  );
  assert.deepEqual(
    facts.find((f) => f.title === "Сервер"),
    { title: "Сервер", value: "Kari" },
  );
  assert.deepEqual(
    facts.find((f) => f.title === "Устройство сервера"),
    { title: "Устройство сервера", value: "cuda:0" },
  );
  assert.deepEqual(
    facts.find((f) => f.title === "Ускорение сервера"),
    { title: "Ускорение сервера", value: "NVIDIA CUDA" },
  );
  assert.equal(
    facts.some((f) => f.title === "Выбранное ускорение"),
    false,
  );
  assert.deepEqual(
    facts.find((f) => f.title === "Распознавание на сервере"),
    { title: "Распознавание на сервере", value: "220 мс" },
  );
  assert.deepEqual(dictationStages({ duration: 5, metadata }), [
    { title: "Подготовка подключения", value: "30 мс" },
    { title: "Запрос к серверу", value: "420 мс" },
    { title: "На сервере, всего", value: "380 мс" },
    { title: "Передача по сети", value: "0 мс" },
  ]);
});

test("old remote entries retain available device facts without calling server total recognition or inventing network", () => {
  for (const entry of [
    {
      device: "remote:cuda:0",
      metadata: {
        backend: null,
        recognition_duration_ms: 800,
        model: "whisper:tiny",
        requested_acceleration: "cuda",
      },
    },
    {
      device: "remote:cuda:0",
      processing: { audio_secs: 3, transcribe_secs: 0.8, ai_mode: "off" },
    },
  ]) {
    const metadata = historyMetadataFromNative(entry);
    assert.equal(metadata.backend, "remote");
    assert.equal(metadata.remoteDevice, "cuda:0");
    assert.equal(metadata.remoteBackend, entry.metadata ? undefined : "cuda");
    assert.equal(metadata.remoteName, undefined);
    assert.ok(metadata.recognitionDurationMs == null);
    assert.equal(metadata.networkDurationMs, undefined);
    assert.equal(metadata.serverTotalDurationMs, undefined);
    const facts = dictationFacts({ duration: 3, metadata });
    assert.equal(
      facts.some((f) => f.title === "Выбранное ускорение"),
      false,
    );
    assert.deepEqual(
      facts.find((f) => f.title === "Распознавание на сервере"),
      { title: "Распознавание на сервере", value: "Не сохранено" },
    );
    assert.equal(
      dictationStages({ duration: 3, metadata }).some(
        (f) => f.title === "Передача по сети",
      ),
      false,
    );
  }
});

test("unknown backend, device and invalid durations remain unknown, including zero-versus-absent semantics", () => {
  const future = historyMetadataFromNative({
    device: "remote:cuda:0",
    metadata: {
      backend: "future_gpu",
      remote_backend: "cuda",
      remote_device: "cuda:0",
    },
  });
  assert.equal(future.backend, undefined);
  assert.equal(future.remoteBackend, undefined);
  const explicitUnknown = historyMetadataFromNative({
    device: "remote:cuda:0",
    metadata: {
      backend: "remote",
      remote_device: "future:0",
      remote_backend: "future_gpu",
    },
  });
  assert.equal(explicitUnknown.remoteDevice, undefined);
  assert.equal(explicitUnknown.remoteBackend, undefined);
  const metadata = historyMetadataFromNative({
    device: "remote:PRIVATE_DEVICE",
    metadata: {
      backend: "remote",
      remote_name: "https://PRIVATE_SERVER",
      remote_device: "PRIVATE_DEVICE",
      remote_backend: "future_gpu",
      request_duration_ms: -1,
      server_total_duration_ms: Number.MAX_SAFE_INTEGER,
      decode_duration_ms: 0.5,
      network_duration_ms: NaN,
    },
  });
  for (const key of [
    "remoteName",
    "remoteDevice",
    "remoteBackend",
    "requestDurationMs",
    "serverTotalDurationMs",
    "decodeDurationMs",
    "networkDurationMs",
  ])
    assert.equal(metadata[key], undefined, key);
  assert.equal(dictationStages({ duration: 1, metadata }).length, 0);
  assert.ok(
    !JSON.stringify(dictationFacts({ duration: 1, metadata })).includes(
      "PRIVATE",
    ),
  );
  assert.equal(
    historyMetadataFromNative({ device: "remote:/PRIVATE", processing: null }),
    undefined,
  );
});

test("post-storage unknown and cleared fields cannot be re-inferred from the legacy device column", () => {
  const unknown = historyMetadataFromNative({
    device: "remote:cuda:0",
    metadata: { backend: "unknown", remote_backend: null, remote_device: null },
  });
  assert.equal(unknown.backend, undefined);
  assert.equal(unknown.remoteBackend, undefined);
  assert.equal(unknown.remoteDevice, undefined);
  const remote = historyMetadataFromNative({
    device: "remote:cuda:0",
    metadata: {
      backend: "remote",
      remote_backend: null,
      remote_device: null,
      decode_duration_ms: null,
      recognition_duration_ms: 999,
    },
  });
  assert.equal(remote.backend, "remote");
  assert.equal(remote.remoteBackend, undefined);
  assert.equal(remote.remoteDevice, undefined);
  assert.equal(remote.recognitionDurationMs, undefined);
  const missing = historyMetadataFromNative({
    device: "remote:cuda:0",
    metadata: { backend: null },
  });
  assert.equal(missing.backend, "remote");
  assert.equal(missing.remoteDevice, "cuda:0");
  assert.equal(missing.remoteBackend, undefined);
});

test("local reserve displays its actual CPU facts and ignores stale remote attempt metadata", () => {
  const metadata = historyMetadataFromNative({
    device: "CPU",
    metadata: {
      backend: "cpu",
      fallback: true,
      remote_name: "Kari",
      remote_device: "cuda:0",
      remote_backend: "cuda",
      request_duration_ms: 400,
      decode_duration_ms: 200,
      network_duration_ms: 0,
      model: "giga_am:v3_e2e_rnnt_int8",
      recognition_duration_ms: 800,
      requested_acceleration: "cpu",
    },
  });
  assert.equal(metadata.backend, "cpu");
  assert.equal(metadata.recognitionDurationMs, 800);
  for (const key of [
    "remoteName",
    "remoteDevice",
    "remoteBackend",
    "requestDurationMs",
    "decodeDurationMs",
    "networkDurationMs",
  ])
    assert.equal(metadata[key], undefined, key);
  const facts = dictationFacts({ duration: 1, metadata });
  assert.deepEqual(
    facts.find((f) => f.title === "Фактический backend"),
    { title: "Фактический backend", value: "CPU" },
  );
  assert.equal(
    facts.some((f) => f.title === "Сервер"),
    false,
  );
  assert.equal(
    facts.some((f) => f.value === "Резерв · локально на CPU"),
    true,
  );
});

test("contradictory remote device facts and durations remain unknown after mapping", () => {
  const metadata = historyMetadataFromNative({
    device: "remote:cuda:0",
    metadata: {
      backend: "remote",
      remote_device: "cuda:0",
      remote_backend: "cpu",
      request_duration_ms: 100,
      server_total_duration_ms: 90,
      decode_duration_ms: 91,
      network_duration_ms: 101,
    },
  });
  assert.equal(metadata.remoteDevice, undefined);
  assert.equal(metadata.remoteBackend, undefined);
  assert.equal(metadata.decodeDurationMs, undefined);
  assert.equal(metadata.recognitionDurationMs, undefined);
  assert.equal(metadata.networkDurationMs, undefined);
  assert.equal(metadata.requestDurationMs, 100);
  assert.equal(metadata.serverTotalDurationMs, 90);
  assert.equal(
    dictationStages({ duration: 1, metadata }).some(
      (stage) => stage.title === "Передача по сети",
    ),
    false,
  );
  const exact = historyMetadataFromNative({
    device: "remote:cpu",
    metadata: {
      backend: "remote",
      remote_device: "cpu",
      remote_backend: "cpu",
      request_duration_ms: 100,
      server_total_duration_ms: 90,
      decode_duration_ms: 90,
      network_duration_ms: 100,
    },
  });
  assert.equal(exact.remoteDevice, "cpu");
  assert.equal(exact.remoteBackend, "cpu");
  assert.equal(exact.decodeDurationMs, 90);
  assert.equal(exact.networkDurationMs, 100);
});

test("history statistics renders remote timing semantics and safely escaped names in the real component", async () => {
  const { default: HistoryStatistics } = await server.ssrLoadModule(
    "/src/v3/features/history/presentation/HistoryStatistics.vue",
  );
  const metadata = historyMetadataFromNative({
    device: "remote:cuda:0",
    metadata: {
      backend: "remote",
      remote_name: "Kari <GPU>",
      remote_device: "cuda:0",
      remote_backend: "cuda",
      model: "whisper:large_turbo",
      request_duration_ms: 410,
      server_total_duration_ms: 380,
      requested_acceleration: "cuda",
    },
  });
  const markup = await renderToString(
    createSSRApp({
      render: () => h(HistoryStatistics, { entry: { duration: 3, metadata } }),
    }),
  );
  assert.match(markup, /Kari &lt;GPU&gt;/);
  assert.match(markup, /Сервер Fono/);
  assert.match(markup, /Запрос к серверу/);
  assert.match(markup, /На сервере, всего/);
  assert.match(markup, /Время сервера входит во время запроса/);
  assert.ok(!markup.includes("Передача по сети"));
  assert.ok(!markup.includes("Выбранное ускорение"));
});
