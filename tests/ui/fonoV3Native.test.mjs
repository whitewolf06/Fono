import assert from "node:assert/strict";
import { before, after, beforeEach, test } from "node:test";
import { createServer } from "vite";
let server,
  mapping,
  createContext,
  nativeSettings,
  nativeDictation,
  rawDefaults,
  defaults;
let calls, invoke;
before(async () => {
  globalThis.window = {
    __TAURI_INTERNALS__: { invoke: (...args) => invoke(...args) },
  };
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { hmr: false, middlewareMode: true },
  });
  mapping = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/mapping.ts",
  );
  ({ createNativeContext: createContext } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/context.ts",
  ));
  ({ nativeSettings } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/settings.ts",
  ));
  ({ nativeDictation } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/dictation.ts",
  ));
  ({ DEFAULT_SETTINGS: rawDefaults } =
    await server.ssrLoadModule("/src/lib/types.ts"));
  ({ defaults } = await server.ssrLoadModule(
    "/src/v3/features/preferences/domain/preferences.ts",
  ));
});
after(async () => {
  await server.close();
  delete globalThis.window;
});
beforeEach(() => {
  calls = [];
  invoke = async (command, args) => {
    calls.push([command, args]);
    if (command === "get_settings") return structuredClone(rawDefaults);
    if (
      [
        "get_dictation_history",
        "list_whisper_models",
        "list_audio_devices",
      ].includes(command)
    )
      return [];
    if (command === "get_wake_word_status") return "listening";
  };
});
function state() {
  return {
    preferences: { ...defaults },
    profiles: [],
    models: [],
    history: [],
    applications: [],
    jobs: [],
    last: {
      entry: null,
      variant: "result",
      draft: "",
      edited: false,
      undo: null,
    },
    phase: "idle",
    audioLevel: 0,
    elapsed: 0,
    error: "",
    pending: {},
    scenario: "normal",
    microphoneAvailable: false,
    aiAvailable: false,
    logs: [],
    onboardingStep: 0,
    testSignal: 0,
  };
}
test("native patches preserve unrelated settings, key flags and overlay coordinates", () => {
  const raw = structuredClone(rawDefaults);
  raw.overlay_x = -1200;
  raw.overlay_y = 70;
  raw.has_llm_api_key = true;
  raw.llm_profiles = [
    {
      id: "secure",
      api_key: null,
      has_api_key: true,
      name: "Saved",
      base_url: "http://localhost:1234/v1",
      provider: "lmstudio",
      connection: "local",
      model: "real-model",
    },
  ];
  const next = mapping.applyPreferences(
    raw,
    {
      overlayScale: 125,
      overlayOpacity: 80,
      microphone: "system",
      hotkey: "Ctrl + Alt + F9",
    },
    [],
  );
  assert.equal(next.overlay_scale, 1.25);
  assert.equal(next.overlay_opacity, 0.8);
  assert.equal(next.overlay_x, -1200);
  assert.equal(next.audio_device_id, null);
  assert.equal(next.hotkey, "Ctrl+Alt+F9");
  assert.deepEqual(next.llm_profiles, raw.llm_profiles);
  assert.equal(raw.overlay_scale, rawDefaults.overlay_scale);
});
test("native trainer consent revocation disables both trainer and cloud analysis", () => {
  const raw = structuredClone(rawDefaults);
  raw.analytics_enabled = true;
  raw.speech_trainer_enabled = true;
  raw.speech_analysis_llm.enabled = true;
  const next = mapping.applyPreferences(raw, { analyticsConsent: false }, []);
  assert.equal(next.speech_trainer_enabled, false);
  assert.equal(next.speech_analysis_llm.enabled, false);
});
test("wake toggle invokes detector lifecycle and rolls back on error", async () => {
  const s = state();
  s.preferences.wakeEnabled = false;
  const port = nativeSettings(createContext(s));
  await port.toggle("wakeEnabled", true);
  assert.equal(calls[0][0], "enable_wake_word");
  assert.ok(!calls.some(([name]) => name === "save_settings"));
  s.preferences.wakeEnabled = false;
  invoke = async () => {
    throw "Модель не найдена";
  };
  await assert.rejects(port.toggle("wakeEnabled", true), /Модель не найдена/);
  assert.equal(s.preferences.wakeEnabled, false);
  assert.equal(s.pending.wakeEnabled, false);
});
test("serialized settings patches read latest settings and do not lose a preceding update", async () => {
  const raw = structuredClone(rawDefaults),
    ctx = createContext(state());
  invoke = async (name, args) => {
    if (name === "get_settings") return structuredClone(raw);
    if (name === "save_settings") {
      await new Promise((resolve) => setTimeout(resolve, 8));
      Object.assign(raw, args.settings);
    }
  };
  await Promise.all([
    ctx.saveRaw((s) => ({ ...s, language: "en" })),
    ctx.saveRaw((s) => ({ ...s, overlay_scale: 1.5 })),
  ]);
  assert.equal(raw.language, "en");
  assert.equal(raw.overlay_scale, 1.5);
});
test("last result is independent from history, and duplicate snapshots do not overwrite edits", () => {
  const s = state(),
    ctx = createContext(s),
    dictation = nativeDictation(ctx);
  s.preferences.historyEnabled = false;
  const result = {
    id: "s1",
    text: "Чистый текст.",
    original_text: "ну чистый текст",
    audio_secs: 4,
    created_at: new Date().toISOString(),
  };
  dictation.accept(result);
  dictation.port.edit("Моя правка");
  dictation.snapshot({
    last: result,
    state: "idle",
    operation_id: 1,
    level: 0,
    source: null,
  });
  assert.equal(s.last.draft, "Моя правка");
  assert.equal(s.last.entry.original, "ну чистый текст");
  assert.equal(s.history.length, 0);
  dictation.port.chooseVariant("original");
  assert.equal(s.last.draft, "ну чистый текст");
});
test("a slow improvement cannot replace a newer dictation; undo restores only the draft", async () => {
  const s = state(),
    dictation = nativeDictation(createContext(s));
  const result = {
    id: "s1",
    text: "Первый текст",
    original_text: "первый текст",
    audio_secs: 2,
    created_at: new Date().toISOString(),
  };
  dictation.accept(result);
  let resolve;
  invoke = async (name) =>
    name === "improve_text"
      ? new Promise((r) => {
          resolve = r;
        })
      : [];
  const pending = dictation.port.improve();
  dictation.accept({ ...result, id: "s2", text: "Новая диктовка" });
  resolve("Старый улучшенный текст");
  await assert.rejects(pending, /Черновик изменился/);
  assert.equal(s.last.draft, "Новая диктовка");
  invoke = async (name) => (name === "improve_text" ? "Улучшенный текст" : []);
  await dictation.port.improve();
  dictation.port.undoImprove();
  assert.equal(s.last.draft, "Новая диктовка");
  assert.equal(s.last.entry.text, "Новая диктовка");
});
test("wake recording is confirmed without racing its auto-stop; hotkey must be released", async () => {
  const s = state(),
    dictation = nativeDictation(createContext(s));
  s.recordingSource = "wake_word";
  await dictation.port.finish();
  assert.equal(calls.at(-1)[0], "confirm_dictation");
  s.recordingSource = "hotkey";
  await assert.rejects(dictation.port.finish(), /Отпустите/);
  s.recordingSource = "ui";
  await dictation.port.finish();
  assert.equal(calls.at(-1)[0], "stop_dictation");
});
test("model deletion cannot cancel a similarly named download", async () => {
  const s = state();
  s.models = [{ id: "large_v3_turbo", status: "downloading" }];
  await nativeSettings(createContext(s)).removeModel("large_v3_turbo");
  assert.deepEqual(calls[0], [
    "cancel_model_download",
    { downloadId: "whisper:large_v3_turbo" },
  ]);
});
