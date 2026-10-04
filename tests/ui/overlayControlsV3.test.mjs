import assert from "node:assert/strict";
import { before, after, beforeEach, test } from "node:test";
import { createSSRApp, h, reactive } from "vue";
import { renderToString } from "vue/server-renderer";
import { createServer } from "vite";

let server,
  createControls,
  createNativeOverlay,
  OverlayPreview,
  defaults,
  rawDefaults;
let invoke, calls, callbacks, listeners;
const flush = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve, reject;
  const promise = new Promise((ok, fail) => {
    resolve = ok;
    reject = fail;
  });
  return { promise, resolve, reject };
}
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
  ({ defaults } = await server.ssrLoadModule(
    "/src/v3/features/preferences/domain/preferences.ts",
  ));
  ({ DEFAULT_SETTINGS: rawDefaults } =
    await server.ssrLoadModule("/src/lib/types.ts"));
  ({ createOverlayControls: createControls } = await server.ssrLoadModule(
    "/src/v3/features/overlay/infrastructure/overlayControls.ts",
  ));
  ({ default: OverlayPreview } = await server.ssrLoadModule(
    "/src/v3/features/overlay/presentation/OverlayPreview.vue",
  ));
  ({ createNativeOverlay } = await server.ssrLoadModule(
    "/src/v3/features/overlay/infrastructure/nativeOverlay.ts",
  ));
  // Native adapter setup needs a class marker, but no native window or DOM is opened.
  globalThis.document = { documentElement: { classList: { add() {} } } };
});
beforeEach(() => {
  calls = [];
  callbacks = new Map();
  listeners = new Map();
  invoke = async (name, args) => {
    calls.push([name, args]);
    if (name === "plugin:event|listen") {
      listeners.set(args.event, callbacks.get(args.handler));
      return args.handler;
    }
    if (name === "get_settings") return structuredClone(rawDefaults);
    if (
      name === "get_overlay_preview" ||
      name === "get_live_dictation" ||
      name === "get_pending_dictation"
    )
      return null;
    if (name === "get_desktop_snapshot")
      return {
        state: "idle",
        operation_id: 0,
        source: null,
        level: 0,
        last: null,
      };
  };
});
after(async () => {
  await server?.close();
  delete globalThis.window;
  delete globalThis.document;
});
const choice = (preset = "clean", targetLanguage = null) => ({
  preset,
  targetLanguage,
});
const pending = (sessionId, patch = {}) => ({
  sessionId,
  phase: "awaiting_action",
  originalText: "Текст для проверки",
  resultText: null,
  createdAt: "2026-10-04T12:00:00Z",
  preset: "clean",
  targetLanguage: null,
  processingEnabled: true,
  source: "hotkey",
  error: null,
  insertionBlocked: false,
  ...patch,
});
function setup(patch = {}) {
  const state = reactive({
    preferences: { ...defaults, hotkeyMode: "toggle" },
    phase: "listening",
    sessionId: 4,
    preview: null,
    pending: null,
    processingChoice: null,
    ...patch,
  });
  let settings = structuredClone(rawDefaults),
    disposed = false;
  const commits = [];
  const controls = createControls(
    state,
    () => settings,
    (value) => {
      settings = value;
      commits.push(value);
    },
    () => disposed,
  );
  return {
    state,
    controls,
    commits,
    dispose() {
      disposed = true;
      controls.dispose();
    },
  };
}
function saveChoices() {
  const saves = [],
    original = invoke;
  invoke = async (name, args) => {
    if (name !== "update_overlay_processing_choice")
      return original(name, args);
    calls.push([name, args]);
    const d = deferred();
    saves.push({ ...d, request: args.request });
    return d.promise;
  };
  return saves;
}
const saved = (preset, targetLanguage = null) => ({
  ...rawDefaults,
  processing_preset: preset,
  processing_target_language: targetLanguage,
});

test("recording controls persist immediately and Finish awaits the latest selected style and translation", async () => {
  const { state, controls, commits, dispose } = setup(),
    saves = saveChoices(),
    stops = [];
  try {
    controls.choose(choice("formal", "en"));
    assert.equal(saves.length, 1);
    assert.deepEqual(saves[0].request, {
      sessionId: 4,
      preset: "formal",
      targetLanguage: "en",
    });
    controls.choose(choice("task", "de"));
    const finishing = controls.finish(async () => stops.push(state.sessionId));
    saves[0].resolve(saved("formal", "en"));
    await flush();
    assert.equal(saves.length, 2);
    assert.deepEqual(stops, []);
    assert.deepEqual(
      commits,
      [],
      "superseded response must not reset the latest choice",
    );
    saves[1].resolve(saved("task", "de"));
    await finishing;
    assert.deepEqual(stops, [4]);
    assert.equal(commits.length, 1);
    assert.deepEqual(state.processingChoice, choice("task", "de"));
  } finally {
    dispose();
  }
});

test("Finish never stops a changed session or an already transcribing/pending operation", async () => {
  for (const change of [
    (s) => {
      s.sessionId = 5;
    },
    (s) => {
      s.phase = "transcribing";
    },
    (s) => {
      s.pending = pending(4);
      s.phase = "awaiting_action";
    },
  ]) {
    const { state, controls, dispose } = setup(),
      saves = saveChoices(),
      stops = [];
    try {
      controls.choose(choice("formal"));
      const finishing = controls.finish(async () => stops.push("stop"));
      change(state);
      saves[0].resolve(saved("formal"));
      await finishing;
      assert.deepEqual(stops, []);
    } finally {
      dispose();
    }
  }
});

test("persistence failure visibly rolls back choice and blocks Finish until a successful retry or Cancel", async () => {
  const { state, controls, dispose } = setup(),
    saves = saveChoices(),
    stops = [];
  try {
    controls.choose(choice("formal", "en"));
    const finishing = controls.finish(async () => stops.push("stop"));
    saves[0].reject(new Error("Не удалось сохранить выбор"));
    await assert.rejects(finishing, /Не удалось сохранить/);
    assert.deepEqual(state.processingChoice, choice());
    assert.match(controls.state.error, /Не удалось сохранить/);
    await assert.rejects(
      controls.finish(async () => stops.push("stop")),
      /Не удалось сохранить/,
    );
    assert.deepEqual(stops, []);
    controls.choose(choice("format", "fr"));
    const retry = controls.finish(async () => stops.push("stop"));
    saves[1].resolve(saved("format", "fr"));
    await retry;
    assert.deepEqual(stops, ["stop"]);
    controls.cancel();
    assert.equal(controls.state.error, "");
  } finally {
    dispose();
  }
});

test("pending choice and resolution belong to the exact pending session; Cancel bypasses failed persistence", async () => {
  const { state, controls, dispose } = setup({
      phase: "awaiting_action",
      pending: pending(12),
      sessionId: null,
    }),
    saves = saveChoices(),
    resolved = [];
  try {
    controls.choose(choice("task", "ru"));
    assert.equal(saves[0].request.sessionId, 12);
    const oldRequest = {
      sessionId: 12,
      action: "process_and_insert",
      preset: "task",
      targetLanguage: "ru",
    };
    const oldAction = controls.resolve(oldRequest, async (r) =>
      resolved.push(r),
    );
    state.pending = pending(13);
    saves[0].resolve(saved("task", "ru"));
    await oldAction;
    assert.deepEqual(resolved, []);
    controls.choose(choice("formal", "en"));
    assert.equal(saves[1].request.sessionId, 13);
    const newRequest = {
      sessionId: 13,
      action: "process_and_insert",
      preset: "formal",
      targetLanguage: "en",
    };
    const newAction = controls.resolve(newRequest, async (r) =>
      resolved.push(r),
    );
    saves[1].resolve(saved("formal", "en"));
    await newAction;
    assert.deepEqual(resolved, [newRequest]);
    controls.choose(choice("format"));
    const failed = controls.resolve(
      { ...newRequest, action: "process_and_insert" },
      async (r) => resolved.push(r),
    );
    saves[2].reject(new Error("Сохранение недоступно"));
    await assert.rejects(failed, /недоступно/);
    const cancel = { sessionId: 13, action: "cancel" };
    await controls.resolve(cancel, async (r) => resolved.push(r));
    assert.deepEqual(resolved.at(-1), cancel);
    assert.equal(controls.state.error, "");
  } finally {
    dispose();
  }
});

test("raw insertion bypasses failed style persistence without processing; stale raw or Cancel cannot discard the current choice", async () => {
  const { state, controls, dispose } = setup({
      phase: "awaiting_action",
      pending: pending(30),
      sessionId: null,
    }),
    saves = saveChoices(),
    resolved = [];
  try {
    controls.choose(choice("formal", "en"));
    const processing = controls.resolve(
      {
        sessionId: 30,
        action: "process_and_insert",
        preset: "formal",
        targetLanguage: "en",
      },
      async (r) => resolved.push(r),
    );
    saves[0].reject(new Error("Выбор не сохранён"));
    await assert.rejects(processing, /не сохранён/);
    for (const action of ["insert_raw", "cancel"]) {
      await controls.resolve({ sessionId: 29, action }, async (r) =>
        resolved.push(r),
      );
      assert.match(
        controls.state.error,
        /не сохранён/,
        "stale actions cannot clear a current session error",
      );
    }
    assert.deepEqual(resolved, []);
    const raw = { sessionId: 30, action: "insert_raw" };
    await controls.resolve(raw, async (r) => resolved.push(r));
    assert.deepEqual(resolved, [raw]);
    assert.equal(controls.state.error, "");
    assert.equal(
      saves.length,
      1,
      "raw fallback performs no new persistence or model work",
    );
    assert.equal(state.pending.sessionId, 30);
  } finally {
    dispose();
  }
});

async function render(patch = {}, props = {}) {
  const app = createSSRApp({
    render: () =>
      h(OverlayPreview, {
        preferences: {
          ...defaults,
          hotkeyMode: "toggle",
          processingEnabled: true,
          overlayQuickProcessing: true,
          ...patch,
        },
        phase: "listening",
        interactive: true,
        elapsed: 31,
        ...props,
      }),
  });
  return renderToString(app);
}
function buttons(html) {
  return [...html.matchAll(/<button\b[^>]*>[\s\S]*?<\/button>/g)].map(
    (m) => m[0],
  );
}

test("recording SSR shows quick choices only for toggle with processing and the optional flag enabled", async () => {
  const enabled = await render();
  assert.match(enabled, /aria-label="Стиль обработки"/);
  assert.match(enabled, /aria-label="Перевод после обработки"/);
  assert.match(enabled, /Выбор сохраняется для следующих диктовок/);
  assert.equal(
    buttons(enabled).filter((b) => b.includes("overlay-finish")).length,
    1,
  );
  assert.match(
    buttons(enabled).find((b) => b.includes("overlay-finish")),
    /Завершить/,
  );
  assert.match(
    buttons(enabled).find((b) => b.includes("overlay-cancel")),
    /Отмена/,
  );
  for (const patch of [
    { hotkeyMode: "hold" },
    { processingEnabled: false },
    { overlayQuickProcessing: false },
  ]) {
    const html = await render(patch);
    assert.doesNotMatch(html, /aria-label="Стиль обработки"/);
    assert.doesNotMatch(html, /aria-label="Перевод после обработки"/);
    assert.equal(
      buttons(html).filter((b) => b.includes("overlay-finish")).length,
      1,
    );
    assert.equal(
      buttons(html).filter((b) => b.includes("overlay-cancel")).length,
      1,
    );
  }
});

test("saving disables only Finish; selected options remain clickable and pending keeps raw/process/copy/cancel", async () => {
  const html = await render(
    {},
    { processingSaving: true, processingChoice: choice("formal", "en") },
  );
  assert.match(
    buttons(html).find((b) => b.includes("overlay-finish")),
    /\bdisabled\b/,
  );
  assert.doesNotMatch(
    buttons(html).find((b) => b.includes("overlay-cancel")),
    /\bdisabled\b/,
  );
  const choices = buttons(html).filter((b) =>
    /dictation-action--(?:process|translate)/.test(b),
  );
  assert.equal(choices.length, 10);
  assert.ok(choices.every((b) => !/\bdisabled\b/.test(b)));
  const pendingHtml = await render(
    {},
    {
      phase: "awaiting_action",
      pending: pending(20, {
        preset: "formal",
        targetLanguage: "en",
        resultText: "Обработанный вариант",
      }),
    },
  );
  assert.match(pendingHtml, /Обработанный вариант/);
  assert.match(pendingHtml, /Исходный/);
  assert.match(pendingHtml, /Обработать · EN/);
  assert.match(pendingHtml, /aria-label="Копировать текст"/);
  assert.match(
    buttons(pendingHtml).find((b) => b.includes("overlay-cancel")),
    /Отмена/,
  );
});

test("native hydration waits for event bindings and never overwrites newer settings or preview events", async () => {
  const oldSettings = deferred(),
    oldPreview = deferred(),
    subscriptions = [],
    getters = [];
  const baseline = invoke;
  invoke = async (name, args) => {
    if (name === "plugin:event|listen") {
      calls.push([name, args]);
      const ready = deferred();
      subscriptions.push(() => {
        listeners.set(args.event, callbacks.get(args.handler));
        ready.resolve(args.handler);
      });
      return ready.promise;
    }
    if (name === "get_settings") {
      getters.push(name);
      return oldSettings.promise;
    }
    if (name === "get_overlay_preview") {
      getters.push(name);
      return oldPreview.promise;
    }
    return baseline(name, args);
  };
  const overlay = createNativeOverlay();
  try {
    await flush();
    assert.deepEqual(
      getters,
      [],
      "hydration must not race subscription installation",
    );
    subscriptions.forEach((ready) => ready());
    await flush();
    assert.ok(getters.includes("get_settings"));
    const fresh = {
      ...rawDefaults,
      hotkey_mode: "toggle",
      overlay_quick_processing: false,
      processing_preset: "formal",
      processing_target_language: "fr",
    };
    listeners.get("settings-changed")({ payload: fresh });
    listeners.get("overlay-preview")({
      payload: {
        overlay_scale: 1.2,
        overlay_opacity: 0.7,
        overlay_mini_mode: true,
      },
    });
    oldSettings.resolve({
      ...rawDefaults,
      hotkey_mode: "hold",
      processing_preset: "clean",
    });
    oldPreview.resolve({
      overlay_scale: 0.7,
      overlay_opacity: 1,
      overlay_mini_mode: false,
    });
    await flush();
    assert.equal(overlay.state.preferences.hotkeyMode, "toggle");
    assert.equal(overlay.state.preferences.overlayQuickProcessing, false);
    assert.equal(overlay.state.preferences.processingMode, "formal");
    assert.equal(overlay.state.preferences.overlayScale, 120);
    assert.equal(overlay.state.preferences.overlayOpacity, 70);
    assert.equal(overlay.state.preferences.overlayCompact, true);
  } finally {
    oldSettings.resolve(rawDefaults);
    oldPreview.resolve(null);
    subscriptions.forEach((ready) => ready());
    overlay.dispose();
    await flush();
  }
});

test("native terminal Close dismisses the overlay; only positive active session IDs can stop/cancel a capture", async () => {
  const overlay = createNativeOverlay();
  try {
    await flush();
    for (const phase of ["idle", "done", "error", "cancelled"]) {
      overlay.state.phase = phase;
      overlay.state.sessionId = null;
      overlay.state.preview = null;
      calls.length = 0;
      await overlay.cancel();
      assert.ok(
        calls.some(([name]) => name === "dismiss_dictation_overlay"),
        `terminal ${phase} Close must dismiss`,
      );
      assert.ok(
        !calls.some(([name]) =>
          /^(stop_dictation|cancel_dictation|confirm_dictation)$/.test(name),
        ),
        "terminal dismiss cannot operate on an unrelated capture",
      );
    }
    overlay.state.phase = "idle";
    overlay.state.preview = {
      overlay_scale: 1,
      overlay_opacity: 1,
      overlay_mini_mode: false,
    };
    calls.length = 0;
    await overlay.finish();
    assert.ok(calls.some(([name]) => name === "dismiss_dictation_overlay"));
    overlay.state.preview = null;
    overlay.state.phase = "listening";
    overlay.state.source = "hotkey";
    for (const sessionId of [null, 0]) {
      overlay.state.sessionId = sessionId;
      calls.length = 0;
      await overlay.finish();
      await overlay.cancel();
      assert.ok(
        !calls.some(([name]) =>
          /^(stop_dictation|cancel_dictation|confirm_dictation)$/.test(name),
        ),
      );
    }
    overlay.state.sessionId = 42;
    calls.length = 0;
    await overlay.finish();
    assert.deepEqual(
      calls.find(([name]) => name === "stop_dictation"),
      ["stop_dictation", { sessionId: 42 }],
    );
    await overlay.cancel();
    assert.deepEqual(
      calls.find(([name]) => name === "cancel_dictation"),
      ["cancel_dictation", { sessionId: 42 }],
    );
  } finally {
    overlay.dispose();
    await flush();
  }
});
