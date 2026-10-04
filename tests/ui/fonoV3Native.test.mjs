import assert from "node:assert/strict";
import { before, after, beforeEach, test } from "node:test";
import { createServer } from "vite";
let server,
  mapping,
  createContext,
  nativeSettings,
  nativeDictation,
  nativeWake,
  createNativeOverlay,
  createPendingOverlay,
  rawDefaults,
  defaults;
let calls, invoke, callbacks, listeners;
before(async () => {
  globalThis.window = {
    __TAURI_INTERNALS__: {
      invoke: (...args) => invoke(...args),
      metadata: { currentWindow: { label: "overlay" } },
      transformCallback: (callback) => {
        const id = callbacks.size + 1;
        callbacks.set(id, callback);
        return id;
      },
    },
    __TAURI_EVENT_PLUGIN_INTERNALS__: {
      unregisterListener: (event) => listeners.delete(event),
    },
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
  ({ nativeWake } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/wake.ts",
  ));
  ({ createNativeOverlay } = await server.ssrLoadModule(
    "/src/v3/features/overlay/infrastructure/nativeOverlay.ts",
  ));
  ({ createPendingOverlay } = await server.ssrLoadModule(
    "/src/v3/features/overlay/infrastructure/pendingOverlay.ts",
  ));
  ({ DEFAULT_SETTINGS: rawDefaults } =
    await server.ssrLoadModule("/src/lib/types.ts"));
  ({ defaults } = await server.ssrLoadModule(
    "/src/v3/features/preferences/domain/preferences.ts",
  ));
  globalThis.document = { documentElement: { classList: { add() {} } } };
});
after(async () => {
  await server?.close();
  delete globalThis.window;
  delete globalThis.document;
});
beforeEach(() => {
  calls = [];
  callbacks = new Map();
  listeners = new Map();
  invoke = async (command, args) => {
    calls.push([command, args]);
    if (command === "plugin:event|listen") {
      listeners.set(args.event, callbacks.get(args.handler));
      return args.handler;
    }
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
    if (command === "is_kws_model_downloaded") return true;
    if (command === "get_wake_word_capabilities")
      return {
        backend: "sherpa_onnx",
        supports_custom_phrase: false,
        supported_phrases: ["hey fono"],
        includes_pre_roll: true,
        supported_languages: ["en"],
        available_languages: ["ru", "en"],
      };
    if (command === "get_wake_calibration_status")
      return {
        active: false,
        recording: false,
        required_samples: 5,
        accepted_samples: 0,
        rejected_samples: 0,
        phrase: "hey fono",
        latest_result: null,
        profile: null,
      };
    if (command === "get_wake_profile_validation_status")
      return {
        active: false,
        recording: false,
        completed: false,
        failed: false,
        positive_passed: 0,
        positive_required: 3,
        silence_passed: false,
        other_phrase_passed: false,
        negative_required: 2,
        latest_result: null,
      };
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

test("legacy wake language follows the phrase alphabet; explicit streaming language wins", () => {
  const raw = structuredClone(rawDefaults);
  for (const backend of [
    "whisper_experimental",
    "sherpa_onnx",
    "disabled",
    "mock",
  ]) {
    raw.wake_backend = backend;
    raw.wake_word = "Привет, компьютер";
    assert.equal(
      mapping.preferencesFromNative(raw, []).wakeLanguage,
      "ru",
      backend,
    );
    raw.wake_word = "Hello, computer";
    assert.equal(
      mapping.preferencesFromNative(raw, []).wakeLanguage,
      "en",
      backend,
    );
  }
  raw.wake_backend = "sherpa_streaming_ru";
  raw.wake_word = "hello computer";
  assert.equal(mapping.preferencesFromNative(raw, []).wakeLanguage, "ru");
  raw.wake_backend = "sherpa_streaming_en";
  raw.wake_word = "привет компьютер";
  assert.equal(mapping.preferencesFromNative(raw, []).wakeLanguage, "en");
});

test("editing a legacy Russian phrase selects RU streaming and rejects mixed scripts before IPC", () => {
  const raw = structuredClone(rawDefaults);
  raw.wake_backend = "whisper_experimental";
  raw.wake_word = "Привет, компьютер";
  const next = mapping.applyPreferences(
    raw,
    { wakePhrase: "Алё, мой помощник" },
    [],
  );
  assert.equal(next.wake_backend, "sherpa_streaming_ru");
  assert.equal(next.wake_word, "Алё, мой помощник");
  assert.throws(
    () => mapping.applyPreferences(raw, { wakePhrase: "Эй, Fono" }, []),
    /кириллицу/,
  );
  assert.throws(
    () =>
      mapping.applyPreferences(
        raw,
        { wakePhrase: "пожалуйста включи запись моего голоса" },
        [],
      ),
    /от 1 до 4 слов/,
  );
  assert.equal(raw.wake_backend, "whisper_experimental");
  assert.equal(raw.wake_word, "Привет, компьютер");
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

test("native idle operation zero completes ordinary dictation and ignores old results", () => {
  const s = state(),
    d = nativeDictation(createContext(s));
  const result = {
    id: "ordinary-1",
    text: "Первый текст",
    original_text: "Первый текст",
    audio_secs: 2,
    created_at: new Date().toISOString(),
  };
  const snapshot = {
    state: "listening",
    level: 0.1,
    last: null,
    operation_id: 1,
    source: "ui",
  };
  d.snapshot(snapshot);
  assert.equal(s.phase, "listening");
  d.accept(result);
  d.snapshot({
    ...snapshot,
    state: "idle",
    operation_id: 0,
    last: result,
    source: null,
  });
  assert.equal(s.phase, "done");
  d.snapshot({ ...snapshot, operation_id: 2 });
  d.accept({ ...result, id: "ordinary-2", text: "Новый текст" });
  d.snapshot({ ...snapshot, state: "idle", operation_id: 0, last: result });
  assert.equal(s.phase, "done");
  assert.equal(s.last.draft, "Новый текст");
});

test("final processed payload updates the same result id without replacing manual edits", () => {
  const s = state(),
    d = nativeDictation(createContext(s));
  const original = {
    id: "ordinary",
    text: "ну текст",
    original_text: "ну текст",
    audio_secs: 2,
    created_at: new Date().toISOString(),
  };
  d.accept(original);
  d.accept({ ...original, text: "Текст." });
  d.accept(original);
  assert.equal(s.last.draft, "Текст.");
  assert.equal(s.last.entry.original, "ну текст");
  d.port.edit("Ручная правка");
  d.accept({ ...original, text: "Обработанный текст." });
  assert.equal(s.last.entry.text, "Обработанный текст.");
  assert.equal(s.last.draft, "Ручная правка");
});
test("wake confirms its auto-stop; overlay Stop finishes both hold and toggle hotkeys", async () => {
  const s = state(),
    dictation = nativeDictation(createContext(s));
  s.recordingSource = "wake_word";
  await dictation.port.finish();
  assert.equal(calls.at(-1)[0], "confirm_dictation");
  s.recordingSource = "hotkey";
  for (const mode of ["hold", "toggle"]) {
    s.preferences.hotkeyMode = mode;
    await dictation.port.finish();
    assert.equal(calls.at(-1)[0], "stop_dictation");
  }
  s.recordingSource = "ui";
  await dictation.port.finish();
  assert.equal(calls.at(-1)[0], "stop_dictation");
});

function pendingDictation(sessionId, patch = {}) {
  return {
    sessionId,
    phase: "awaiting_action",
    originalText: "Публичный пример текста",
    resultText: null,
    createdAt: "2026-10-04T12:00:00Z",
    preset: "clean",
    targetLanguage: null,
    processingEnabled: true,
    source: "hotkey",
    error: null,
    insertionBlocked: false,
    ...patch,
  };
}
function pendingState() {
  return { pending: null, phase: "idle", source: null, level: 0, error: "" };
}
const flush = () => new Promise((resolve) => setImmediate(resolve));

test("overlay hydration and poll replies cannot erase newer pending events", () => {
  const s = pendingState(),
    controller = createPendingOverlay(s, () => false);
  const hydration = controller.stamp();
  controller.apply(pendingDictation(1));
  controller.apply(null, hydration);
  assert.equal(s.pending.sessionId, 1);
  const poll = controller.stamp();
  controller.apply(pendingDictation(2));
  controller.apply(pendingDictation(1), poll);
  controller.apply(null, poll);
  assert.equal(s.pending.sessionId, 2);
  assert.equal(s.phase, "awaiting_action");
  controller.apply(null);
  controller.apply(pendingDictation(2));
  controller.apply(pendingDictation(1));
  assert.equal(
    s.pending,
    null,
    "retired pending cannot resurrect after completion",
  );
});

test("overlay ignores a delayed action error or reply for an older session", async () => {
  const s = pendingState(),
    controller = createPendingOverlay(s, () => false);
  controller.apply(pendingDictation(1));
  let reject;
  invoke = async () =>
    new Promise((_, r) => {
      reject = r;
    });
  const first = controller.resolve({
    sessionId: 1,
    action: "process_and_insert",
    preset: "formal",
    targetLanguage: "en",
  });
  assert.equal(s.phase, "processing");
  controller.apply(null);
  controller.apply(pendingDictation(2));
  reject("Поздняя ошибка предыдущего текста");
  await first;
  assert.equal(s.pending.sessionId, 2);
  assert.equal(s.error, "");
  let resolve;
  invoke = async () =>
    new Promise((r) => {
      resolve = r;
    });
  const second = controller.resolve({ sessionId: 2, action: "insert_raw" });
  controller.apply(null);
  controller.apply(pendingDictation(3));
  resolve(null);
  await second;
  assert.equal(s.pending.sessionId, 3);
});

test("overlay pending errors retain text, duplicate actions are fenced, cancel blocks late work", async () => {
  const s = pendingState(),
    controller = createPendingOverlay(s, () => false);
  controller.apply(pendingDictation(7));
  invoke = async (name, args) => {
    calls.push([name, args]);
    if (name === "resolve_pending_dictation") throw "Соединение недоступно";
  };
  await controller.resolve({ sessionId: 7, action: "process_and_insert" });
  assert.equal(s.pending.originalText, "Публичный пример текста");
  assert.equal(s.phase, "awaiting_action");
  assert.equal(s.error, "Соединение недоступно");
  controller.apply(pendingDictation(7), controller.stamp());
  assert.equal(
    s.error,
    "Соединение недоступно",
    "polling keeps an actionable IPC error visible",
  );
  let finish;
  invoke = async (name, args) => {
    calls.push([name, args]);
    if (args.request.action !== "cancel")
      return new Promise((r) => {
        finish = r;
      });
  };
  const processing = controller.resolve({
    sessionId: 7,
    action: "process_and_insert",
  });
  controller.apply(pendingDictation(7), controller.stamp());
  assert.equal(
    s.phase,
    "processing",
    "a lagging poll cannot re-enable processing controls",
  );
  const before = calls.length;
  await controller.resolve({ sessionId: 7, action: "insert_raw" });
  assert.equal(calls.length, before);
  await controller.resolve({ sessionId: 7, action: "cancel" });
  finish(null);
  await processing;
  assert.equal(s.pending, null);
  assert.equal(s.phase, "cancelled");
  controller.apply(pendingDictation(7));
  assert.equal(s.pending, null);
});

test("native overlay hydrates pending, wires IPC actions/copy, and rejects late initial null", async () => {
  const originalInvoke = invoke;
  let completeHydration;
  invoke = async (name, args) => {
    if (name === "get_pending_dictation")
      return new Promise((r) => {
        completeHydration = r;
      });
    return originalInvoke(name, args);
  };
  const overlay = createNativeOverlay();
  try {
    await flush();
    listeners.get("pending-dictation")({
      payload: pendingDictation(11, { resultText: "Обработанный результат" }),
    });
    completeHydration(null);
    await flush();
    assert.equal(overlay.state.pending.sessionId, 11);
    assert.equal(overlay.state.phase, "awaiting_action");
    invoke = originalInvoke;
    await overlay.copy("Обработанный результат");
    assert.deepEqual(calls.at(-1), [
      "copy_dictation_text",
      { text: "Обработанный результат" },
    ]);
    await overlay.resolve({ sessionId: 11, action: "insert_raw" });
    assert.ok(
      calls.some(
        ([name, args]) =>
          name === "resolve_pending_dictation" && args.request.sessionId === 11,
      ),
    );
    assert.equal(overlay.state.pending, null);
    assert.equal(overlay.state.phase, "done");
    const dictationActions = () =>
      calls.filter(([name]) =>
        ["stop_dictation", "confirm_dictation", "cancel_dictation"].includes(
          name,
        ),
      );
    const actionsBeforeTerminalFinish = dictationActions().length;
    await overlay.finish();
    assert.equal(
      dictationActions().length,
      actionsBeforeTerminalFinish,
      "completed pending text cannot stop an absent recording",
    );
    await overlay.cancel();
    assert.ok(calls.some(([name]) => name === "dismiss_dictation_overlay"));

    for (const [index, mode] of ["hold", "toggle"].entries()) {
      const sessionId = 12 + index;
      Object.assign(overlay.state, {
        phase: "listening",
        sessionId,
        source: "hotkey",
      });
      overlay.state.preferences.hotkeyMode = mode;
      const actionsBeforeStop = dictationActions().length;
      await overlay.finish();
      assert.deepEqual(dictationActions().slice(actionsBeforeStop), [
        ["stop_dictation", { sessionId }],
      ]);
    }

    Object.assign(overlay.state, {
      phase: "idle",
      sessionId: null,
      preview: {
        overlay_scale: 1,
        overlay_opacity: 1,
        overlay_mini_mode: false,
      },
    });
    const dismisses = () =>
      calls.filter(([name]) => name === "dismiss_dictation_overlay").length;
    const dismissesBeforePreview = dismisses();
    const actionsBeforePreview = dictationActions().length;
    await overlay.finish();
    await overlay.cancel();
    assert.equal(dismisses(), dismissesBeforePreview + 2);
    assert.equal(dictationActions().length, actionsBeforePreview);
  } finally {
    overlay.dispose();
    await flush();
  }
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

function liveSnapshot(session = "1", revision = 1, patch = {}) {
  return {
    session_id: session,
    revision,
    committed_text: "Да, да, оставим повторы.",
    draft_text: "Следующая мысль",
    pending_text: "Да, да, оставим повторы.",
    insertion_state: "paused_focus",
    lag_ms: 2400,
    phase: "listening",
    source: "hotkey",
    elapsed_ms: 6000,
    audio_level: 0.5,
    ...patch,
  };
}

test("live snapshots preserve repetitions, reject stale revisions and retired sessions", () => {
  const s = state(),
    d = nativeDictation(createContext(s));
  s.preferences.dictationMode = "live";
  d.live(liveSnapshot("1", 2));
  d.live(liveSnapshot("1", 1, { committed_text: "Старый текст" }));
  assert.equal(s.live.committedText, "Да, да, оставим повторы.");
  assert.equal(s.live.insertionState, "paused_focus");
  assert.equal(s.phase, "listening");
  d.live(liveSnapshot("2", 1, { committed_text: "Новая запись" }));
  d.live(
    liveSnapshot("1", 10, {
      phase: "done",
      committed_text: "Поздний результат",
    }),
  );
  assert.equal(s.live.committedText, "Новая запись");
  assert.ok(!calls.some(([name]) => /inject|improve_text/.test(name)));
});

test("live hotkey finishes explicitly; editing and AI cannot run during capture", async () => {
  const s = state(),
    d = nativeDictation(createContext(s));
  s.preferences.dictationMode = "live";
  d.live(liveSnapshot());
  assert.throws(() => d.port.edit("Изменение"), /завершите/);
  await assert.rejects(d.port.improve(), /завершите/);
  await d.port.finish();
  assert.equal(calls.at(-1)[0], "stop_dictation");
  await d.port.resumeInsertion();
  assert.equal(calls.at(-1)[0], "resume_live_insertion");
  d.live(liveSnapshot("1", 2, { phase: "done" }));
  await assert.rejects(d.port.improve(), /ИИ отключена/);
  assert.ok(!calls.some(([name]) => name === "improve_text"));
});

test("an old desktop snapshot cannot reset live phase or elapsed time", () => {
  const s = state(),
    d = nativeDictation(createContext(s));
  s.preferences.dictationMode = "live";
  d.live(liveSnapshot("2", 3));
  d.snapshot({
    state: "idle",
    level: 0,
    last: null,
    operation_id: 1,
    source: null,
  });
  assert.equal(s.phase, "listening");
  assert.equal(s.elapsed, 6);
  assert.equal(s.recordingSource, "hotkey");
  d.live(liveSnapshot("2", 3, { elapsed_ms: 6800, audio_level: 0.7 }));
  assert.equal(s.elapsed, 6.8);
  assert.equal(s.audioLevel, 0.7);
});

test("uncertain native insertion is never retried automatically", async () => {
  const s = state(),
    d = nativeDictation(createContext(s));
  s.preferences.dictationMode = "live";
  d.live(liveSnapshot("1", 1, { insertion_state: "failed" }));
  await assert.rejects(d.port.resumeInsertion(), /вручную/);
  assert.ok(!calls.some(([name]) => name === "resume_live_insertion"));
});

test("legacy and submitted live mode normalize to classic without changing saved AI preferences", () => {
  const raw = structuredClone(rawDefaults);
  raw.dictation_mode = "live";
  raw.ai_mode = "format";
  assert.equal(
    mapping.preferencesFromNative(raw, []).dictationMode,
    "standard",
  );
  assert.equal(
    mapping.applyPreferences(raw, { overlayScale: 120 }, []).dictation_mode,
    "standard",
  );
  const next = mapping.applyPreferences(
    raw,
    { dictationMode: "live", wakeLanguage: "ru", wakePhrase: "привет фоно" },
    [],
  );
  assert.equal(next.dictation_mode, "standard");
  assert.equal(next.wake_backend, "sherpa_streaming_ru");
  assert.equal(next.wake_word, "привет фоно");
  assert.equal(next.ai_mode, raw.ai_mode);
  const mapped = mapping.preferencesFromNative(next, []);
  assert.equal(mapped.wakeLanguage, "ru");
  assert.equal(mapped.dictationMode, "standard");
});

test("wake test records audio and invokes the detector, and language options include both engines", async () => {
  const s = state(),
    port = nativeWake(createContext(s));
  const originalInvoke = invoke;
  invoke = async (command, args) => {
    if (command === "recognize_wake_word_sample") {
      calls.push([command, args]);
      return { detected: true, recognized: "hey fono", processing_ms: 42 };
    }
    return originalInvoke(command, args);
  };
  await port.load();
  assert.equal(s.wakeSetup.required, 5);
  assert.equal(s.wakeSetup.validation.positiveRequired, 3);
  assert.deepEqual(
    s.wakeCapabilities.languages.map((l) => l.value),
    ["ru", "en"],
  );
  const result = await port.test();
  assert.match(result, /42 мс/);
  const names = calls.map(([name]) => name);
  assert.ok(
    names.indexOf("record_wake_word_sample") <
      names.indexOf("recognize_wake_word_sample"),
  );
  await port.validate("silence");
  assert.ok(
    calls.some(
      ([name, args]) =>
        name === "record_wake_profile_validation_sample" &&
        args.kind === "silence",
    ),
  );
});
