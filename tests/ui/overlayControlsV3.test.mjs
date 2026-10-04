import assert from "node:assert/strict";
import { before, after, beforeEach, test } from "node:test";
import { createSSRApp, h, reactive } from "vue";
import { renderToString } from "vue/server-renderer";
import { createServer } from "vite";

let server,
  createControls,
  createNativeOverlay,
  bindLayout,
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
  ({ bindOverlayLayout: bindLayout } = await server.ssrLoadModule(
    "/src/v3/features/overlay/infrastructure/overlayLayout.ts",
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
    assert.deepEqual(state.processingChoice, {
      ...choice(),
      processingEnabled: true,
      translationEnabled: rawDefaults.processing_translation_enabled,
    });
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

test("recording shows switches even with processing off so it can be enabled during toggle capture", async () => {
  const enabled = await render();
  assert.match(enabled, /aria-label="Выбрать стиль обработки"/);
  assert.match(enabled, /aria-label="Выбрать язык перевода"/);
  assert.equal(
    buttons(enabled).filter((b) => b.includes("overlay-sketch-action accept"))
      .length,
    1,
  );
  assert.match(
    buttons(enabled).find((b) => b.includes("overlay-sketch-action accept")),
    /Завершить/,
  );
  assert.match(
    buttons(enabled).find((b) => b.includes("overlay-sketch-action cancel")),
    /Отменить запись/,
  );
  for (const patch of [
    { hotkeyMode: "hold" },
    { overlayQuickProcessing: false },
  ]) {
    const html = await render(patch);
    assert.doesNotMatch(html, /aria-label="Выбрать стиль обработки"/);
    assert.doesNotMatch(html, /aria-label="Выбрать язык перевода"/);
    assert.equal(
      buttons(html).filter((b) => b.includes("overlay-sketch-action accept"))
        .length,
      1,
    );
    assert.equal(
      buttons(html).filter((b) => b.includes("overlay-sketch-action cancel"))
        .length,
      1,
    );
  }
  const off = await render({ processingEnabled: false });
  assert.match(off, /aria-label="Постобработка"/);
  assert.match(off, /aria-label="Перевод"[^>]*disabled/);
});

test("saving blocks Finish and switches but Cancel remains available; ready result offers Copy and Close only", async () => {
  const html = await render(
    {},
    { processingSaving: true, processingChoice: choice("formal", "en") },
  );
  assert.match(
    buttons(html).find((b) => b.includes("overlay-sketch-action accept")),
    /\bdisabled\b/,
  );
  assert.doesNotMatch(
    buttons(html).find((b) => b.includes("overlay-sketch-action cancel")),
    /\bdisabled\b/,
  );
  const choices = buttons(html).filter((b) =>
    b.includes("overlay-sketch-option"),
  );
  assert.equal(choices.length, 2);
  assert.ok(choices.every((b) => /\bdisabled\b/.test(b)));
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
  assert.match(pendingHtml, /aria-label="Скопировать текст и закрыть"/);
  assert.doesNotMatch(pendingHtml, /overlay-sketch-action accept/);
  assert.match(
    buttons(pendingHtml).find((b) =>
      b.includes("overlay-sketch-action cancel"),
    ),
    /Закрыть без копирования/,
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
      calls.find(([name]) => name === "finish_overlay_dictation"),
      ["finish_overlay_dictation", { sessionId: 42 }],
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

test("physical shortcut acknowledgement waits for the latest persisted overlay switches", async () => {
  const overlay = createNativeOverlay();
  try {
    await flush();
    const saves = saveChoices();
    overlay.state.phase = "listening";
    overlay.state.sessionId = 42;
    overlay.chooseProcessing({
      ...choice("raw", "en"),
      processingEnabled: true,
      translationEnabled: false,
    });
    listeners.get("overlay-processing-flush")({
      payload: { sessionId: 42, requestId: 7 },
    });
    await flush();
    assert.ok(
      !calls.some(([name]) => name === "acknowledge_overlay_processing_flush"),
    );
    saves[0].resolve({
      ...saved("raw", "en"),
      ai_mode: "clean",
      processing_translation_enabled: false,
    });
    await flush();
    assert.deepEqual(
      calls.find(([name]) => name === "acknowledge_overlay_processing_flush"),
      [
        "acknowledge_overlay_processing_flush",
        { sessionId: 42, requestId: 7, error: null },
      ],
    );
    assert.equal(overlay.state.processingChoice.translationEnabled, false);
    assert.equal(overlay.state.processingChoice.targetLanguage, "en");
    assert.ok(
      !calls.some(([name]) => name === "finish_overlay_dictation"),
      "acknowledgement cannot finish or switch a session itself",
    );
  } finally {
    overlay.dispose();
    await flush();
  }
});

test("failed clipboard copy retains the exact pending result and only successful Copy completes without insertion", async () => {
  const overlay = createNativeOverlay();
  try {
    await flush();
    overlay.state.pending = pending(52, {
      copyOnly: true,
      resultText: "готовый текст",
    });
    overlay.state.phase = "awaiting_action";
    const baseline = invoke;
    let failure = true;
    invoke = async (name, args) => {
      if (name === "copy_dictation_text") {
        calls.push([name, args]);
        if (failure) throw new Error("Буфер недоступен");
        return;
      }
      return baseline(name, args);
    };
    await overlay.copy("готовый текст");
    assert.equal(overlay.state.pending.sessionId, 52);
    assert.equal(overlay.state.pending.resultText, "готовый текст");
    assert.match(overlay.state.error, /Буфер недоступен/);
    assert.ok(!calls.some(([name]) => name === "resolve_pending_dictation"));
    failure = false;
    await overlay.copy("готовый текст");
    assert.deepEqual(
      calls.find(([name]) => name === "resolve_pending_dictation"),
      [
        "resolve_pending_dictation",
        { request: { sessionId: 52, action: "complete" } },
      ],
    );
    assert.equal(overlay.state.pending, null);
    assert.equal(overlay.state.copying, false);
    assert.ok(!calls.some(([name]) => name === "reinsert_dictation"));
  } finally {
    overlay.dispose();
    await flush();
  }
});

test("a late clipboard response cannot complete or close a successor pending session", async () => {
  const overlay = createNativeOverlay(),
    copied = deferred();
  try {
    await flush();
    overlay.state.pending = pending(61, { resultText: "старый" });
    const baseline = invoke;
    invoke = async (name, args) =>
      name === "copy_dictation_text" ? copied.promise : baseline(name, args);
    const copying = overlay.copy("старый");
    overlay.state.pending = pending(62, { resultText: "новый" });
    copied.resolve();
    await copying;
    assert.equal(overlay.state.pending.sessionId, 62);
    assert.ok(!calls.some(([name]) => name === "resolve_pending_dictation"));
  } finally {
    copied.resolve();
    overlay.dispose();
    await flush();
  }
});

test("native layout measures the mounted widget, expands before measurement and disposes its observer", async () => {
  const previousDocument = globalThis.document;
  let height = 126,
    notify,
    disconnected = 0,
    observed;
  const widget = { getBoundingClientRect: () => ({ height }) };
  globalThis.document = {
    querySelector: (selector) => {
      assert.equal(selector, ".native-overlay .overlay-sketch");
      return widget;
    },
  };
  globalThis.ResizeObserver = class {
    constructor(callback) {
      notify = callback;
    }
    observe(element) {
      observed = element;
    }
    disconnect() {
      disconnected++;
    }
  };
  const state = reactive({
    preferences: { ...defaults, overlayScale: 150 },
    phase: "listening",
    pending: null,
    helpOpen: false,
    error: "",
  });
  const release = bindLayout(
    state,
    () => "",
    (error) => {
      throw error;
    },
  );
  try {
    await flush();
    assert.equal(observed, widget);
    const layouts = () =>
      calls
        .filter(([name]) => name === "set_overlay_layout")
        .map(([, args]) => args.layout);
    assert.equal(layouts()[0].contentHeight, null);
    assert.equal(
      layouts().at(-1).contentHeight,
      126,
      "getRect already includes the 150% zoom",
    );
    const baseline = layouts().length;
    notify();
    notify();
    await flush();
    assert.equal(
      layouts().length,
      baseline,
      "unchanged bounds do not cause resize loops",
    );
    height = 274;
    state.helpOpen = true;
    await flush();
    assert.deepEqual(
      layouts()
        .slice(baseline)
        .map((layout) => layout.contentHeight),
      [null, 274],
    );
    height = 94;
    state.helpOpen = false;
    await flush();
    assert.equal(layouts().at(-1).contentHeight, 94);
    const beforeDispose = layouts().length;
    release();
    assert.equal(disconnected, 1);
    height = 300;
    notify();
    state.helpOpen = true;
    await flush();
    assert.equal(layouts().length, beforeDispose);
  } finally {
    release();
    globalThis.document = previousDocument;
    delete globalThis.ResizeObserver;
  }
});

test("native layout ignores invalid renderer measurements and retains its static fallback", async () => {
  const previousDocument = globalThis.document;
  globalThis.document = {
    querySelector: () => ({
      getBoundingClientRect: () => ({ height: Number.NaN }),
    }),
  };
  const state = reactive({
    preferences: { ...defaults },
    phase: "listening",
    pending: null,
    helpOpen: false,
    error: "",
  });
  const release = bindLayout(
    state,
    () => "",
    (error) => {
      throw error;
    },
  );
  try {
    await flush();
    const layouts = calls.filter(([name]) => name === "set_overlay_layout");
    assert.equal(layouts.length, 1);
    assert.equal(layouts[0][1].layout.contentHeight, null);
  } finally {
    release();
    globalThis.document = previousDocument;
  }
});
