import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer } from "vite";
import { createSSRApp, h } from "vue";
import { renderToString } from "@vue/server-renderer";

let server,
  messages,
  createMockTrainer,
  createNativeTrainer,
  createSettingsPort,
  defaults,
  validatePreferences,
  mapping,
  rawDefaults,
  browser;
before(async () => {
  server = await createServer({
    appType: "custom",
    server: { middlewareMode: true, hmr: false },
  });
  messages = await server.ssrLoadModule("/src/v3/shared/domain/messages.ts");
  ({ createMockTrainer } = await server.ssrLoadModule(
    "/src/v3/features/trainer/infrastructure/mockTrainer.ts",
  ));
  ({ createNativeTrainer } = await server.ssrLoadModule(
    "/src/v3/features/trainer/infrastructure/nativeTrainer.ts",
  ));
  ({ createSettingsPort } = await server.ssrLoadModule(
    "/src/v3/features/preferences/infrastructure/mockSettings.ts",
  ));
  ({ defaults, validatePreferences } = await server.ssrLoadModule(
    "/src/v3/features/preferences/domain/preferences.ts",
  ));
  mapping = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/mapping.ts",
  );
  ({ DEFAULT_SETTINGS: rawDefaults } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/ipcTypes.ts",
  ));
  browser = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/browser.ts",
  );
});
after(async () => {
  await server?.close();
  delete globalThis.localStorage;
});

test("dictionary keeps Russian defaults and diagnoses missing keys and parameters", () => {
  assert.equal(messages.message("common.copy"), "Копировать");
  assert.equal(
    messages.message("pending.processLanguage", { language: "EN" }),
    "Обработать · EN",
  );
  assert.throws(
    () => messages.message("unknown"),
    (error) => error.code === "missing_message" && error.key === "unknown",
  );
  assert.throws(
    () => messages.message("pending.processLanguage"),
    (error) =>
      error.code === "missing_parameter" &&
      error.key === "pending.processLanguage.language",
  );
  assert.equal(
    messages.message("pending.processLanguage", { language: "{untrusted}" }),
    "Обработать · {untrusted}",
  );
});

const now = Date.parse("2026-10-12T12:00:00Z");
function state() {
  return {
    preferences: {
      ...structuredClone(defaults),
      trainerEnabled: true,
      analyticsConsent: true,
      retentionDays: 90,
      trainerAiEnabled: true,
      trainerProfile: "local",
      trainerModel: "test",
      trainerScope: "original_text",
    },
    profiles: [
      {
        id: "local",
        name: "Local fixture",
        model: "test",
        location: "local",
        url: "http://localhost:1234",
      },
    ],
    aiAvailable: true,
    history: [1, 2, 3].map((id) => ({
      id: String(id),
      title: String(id),
      text: "Result",
      original:
        "Ну, основная мысль выражена понятно и завершается после короткой паузы. ".repeat(
          8,
        ),
      createdAt: "2026-10-12T10:00:00Z",
      duration: 10,
    })),
  };
}
function gate() {
  let release;
  const promise = new Promise((resolve) => {
    release = resolve;
  });
  return { promise, release };
}
async function flush() {
  for (let i = 0; i < 8; i++) await Promise.resolve();
}
function trainerHarness(mode, wait = async () => {}) {
  const source = state();
  const backend = createMockTrainer(source, () => now, wait);
  const calls = [];
  const port =
    mode === "mock"
      ? backend.port
      : createNativeTrainer(
          async (command, args) => {
            calls.push([command, args]);
            if (command === "get_trainer_report") {
              assert.deepEqual(Object.keys(args).sort(), ["days", "refresh"]);
              return backend.port.getReport(args.days, args.refresh);
            }
            if (command === "analyze_trainer_period") {
              assert.deepEqual(Object.keys(args).sort(), [
                "days",
                "fingerprint",
                "requestId",
                "shareText",
              ]);
              assert.equal(args.requestId, "request-1");
              return backend.port.analyzePeriod(args);
            }
            if (command === "cancel_trainer_period") {
              assert.deepEqual(args, { requestId: "request-1" });
              return backend.port.cancelPeriod();
            }
            if (command === "clear_speech_analytics")
              return backend.port.clear();
            assert.fail(`unexpected trainer command ${command}`);
          },
          async () => {},
          () => "request-1",
        );
  return { source, port, calls };
}

// Shared business fixtures exercise both port implementations. The native side
// checks IPC command/argument translation; it does not execute Rust or an LLM.
for (const mode of ["mock", "native adapter"]) {
  test(`${mode}: reads retain a pinned snapshot until explicit refresh`, async () => {
    const { source, port } = trainerHarness(
      mode === "mock" ? "mock" : "native",
    );
    const first = await port.getReport(7);
    assert.equal(first.aiSummary, null);
    source.history.push({ ...source.history[0], id: "new" });
    const stale = await port.getReport(7);
    assert.equal(stale.stale, true);
    assert.equal(stale.snapshot.fingerprint, first.snapshot.fingerprint);
    const fresh = await port.getReport(7, true);
    assert.equal(fresh.stale, false);
    assert.notEqual(fresh.snapshot.fingerprint, first.snapshot.fingerprint);
  });
  test(`${mode}: text sharing stays explicit and metrics are the default`, async () => {
    const { port } = trainerHarness(mode === "mock" ? "mock" : "native");
    const base = await port.getReport(7);
    const numeric = await port.analyzePeriod({
      days: 7,
      fingerprint: base.snapshot.fingerprint,
      shareText: false,
    });
    assert.equal(numeric.aiSummary.dataScope, "metrics_only");
    assert.equal(numeric.aiSummary.context.sharedWords, 0);
    const shared = await port.analyzePeriod({
      days: 7,
      fingerprint: base.snapshot.fingerprint,
      shareText: true,
    });
    assert.equal(shared.aiSummary.dataScope, "original_text");
    assert.ok(shared.aiSummary.context.sharedWords > 0);
  });
  test(`${mode}: request reservation rejects immediate duplicates and cancellation cannot save a late answer`, async () => {
    const pending = gate();
    const { port } = trainerHarness(
      mode === "mock" ? "mock" : "native",
      () => pending.promise,
    );
    const base = await port.getReport(7);
    const input = {
      days: 7,
      fingerprint: base.snapshot.fingerprint,
      shareText: false,
    };
    const first = port.analyzePeriod(input);
    await assert.rejects(port.analyzePeriod(input), /Дождитесь/);
    await flush();
    await port.cancelPeriod();
    pending.release();
    await assert.rejects(first, /отменён/);
    assert.equal((await port.getReport(7)).aiSummary, null);
  });
  test(`${mode}: clearing analytics removes source text and a saved summary`, async () => {
    const { port, source } = trainerHarness(
      mode === "mock" ? "mock" : "native",
    );
    const report = await port.getReport(7);
    await port.analyzePeriod({
      days: 7,
      fingerprint: report.snapshot.fingerprint,
      shareText: false,
    });
    await port.clear();
    assert.ok(
      source.history.every(
        (entry) => !entry.original && entry.analysisReady === false,
      ),
    );
    assert.equal((await port.getReport(7)).aiSummary, null);
  });
}

test("decoding settings retain fast/automatic defaults, map explicit options and sanitize stored demo values", () => {
  assert.equal(validatePreferences(structuredClone(defaults)), null);
  for (const whisperThreads of [0, 9, 1.5, NaN, "3"])
    assert.match(
      validatePreferences({ ...defaults, whisperThreads }),
      /от 1 до 8/,
    );
  assert.match(
    validatePreferences({ ...defaults, whisperDecoding: "unexpected" }),
    /режим распознавания/,
  );
  const old = structuredClone(rawDefaults);
  delete old.whisper_decoding;
  delete old.whisper_threads;
  const legacy = mapping.preferencesFromNative(old, []);
  assert.equal(legacy.whisperDecoding, "fast");
  assert.equal(legacy.whisperThreads, null);
  const native = mapping.applyPreferences(
    old,
    { whisperDecoding: "accurate", whisperThreads: 3 },
    [],
  );
  assert.equal(native.whisper_decoding, "accurate");
  assert.equal(native.whisper_threads, 3);
  const restored = mapping.preferencesFromNative(native, []);
  assert.equal(restored.whisperDecoding, "accurate");
  assert.equal(restored.whisperThreads, 3);
  let stored = "{}";
  globalThis.localStorage = {
    getItem: () => stored,
    setItem: (_, value) => {
      stored = value;
    },
  };
  browser.persistPreferences({
    ...defaults,
    whisperDecoding: "accurate",
    whisperThreads: 3,
  });
  assert.equal(browser.readPreferences().whisperThreads, 3);
  stored = JSON.stringify({
    whisperDecoding: "unexpected",
    whisperThreads: 99,
    original: "private",
    processingPrompts: { clean: { customPrompt: "private" } },
  });
  const safe = browser.readPreferences();
  assert.equal(safe.whisperDecoding, "fast");
  assert.equal(safe.whisperThreads, null);
  assert.equal(safe.original, undefined);
});

test("split report keeps overview, consent and dictation panels mounted in the same report", async () => {
  const { default: PeriodReport } = await server.ssrLoadModule(
    "/src/v3/features/trainer/presentation/PeriodReport.vue",
  );
  const report = await trainerHarness("mock").port.getReport(7);
  for (const view of ["trainer-overview", "trainer-dictations", "trainer-ai"]) {
    const app = createSSRApp({
      render: () =>
        h(
          PeriodReport,
          {
            report,
            loading: false,
            analyzing: false,
            cancelling: false,
            error: "",
            native: false,
            view,
          },
          { dictations: () => h("p", "Dictation fixture") },
        ),
    });
    app.component("RouterLink", {
      props: ["to"],
      setup:
        (props, { slots }) =>
        () =>
          h("a", { href: props.to }, slots.default?.()),
    });
    const html = await renderToString(app);
    assert.match(html, /Демонстрация/);
    assert.match(html, /Dictation fixture/);
    assert.match(html, /Проанализировать период/);
    assert.match(html, /По дням/);
    assert.match(html, /Чаще встречаются/);
    assert.ok(!html.includes("<style"));
  }
});

test("demo decoding save cannot commit if a capture begins during its asynchronous save", async () => {
  let writes = 0;
  globalThis.localStorage = {
    getItem: () => "{}",
    setItem: () => {
      writes++;
    },
  };
  for (const patch of [
    { whisperDecoding: "accurate" },
    { whisperThreads: 3 },
  ]) {
    const source = { ...state(), phase: "idle", scenario: "normal", logs: [] };
    const port = createSettingsPort(source);
    const saved = port.save(patch);
    source.phase = "listening";
    await assert.rejects(saved, /Сначала завершите/);
    assert.equal(source.preferences.whisperDecoding, "fast");
    assert.equal(source.preferences.whisperThreads, null);
  }
  assert.equal(writes, 0);
});
