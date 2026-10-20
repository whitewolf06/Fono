import assert from "node:assert/strict";
import { before, after, beforeEach, afterEach, test } from "node:test";
import { createServer } from "vite";
import { createApp, createSSRApp, h, effectScope, nextTick } from "vue";
import { renderToString } from "@vue/server-renderer";
import { WlConfig, WlToastService } from "@whitelife-core/ui-kit";
let server, createWorkspace, workspace, storage;
before(async () => {
  storage = new Map();
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (key) => storage.get(key) ?? null,
      setItem: (key, value) => storage.set(key, value),
    },
  });
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    define: { __FONO_EXPERIMENTAL_LIVE__: true },
    server: { hmr: false, middlewareMode: true },
  });
  ({ createMockWorkspace: createWorkspace } = await server.ssrLoadModule(
    "/src/v3/app/mockWorkspace.ts",
  ));
});
beforeEach(() => {
  storage.clear();
  workspace = createWorkspace();
});
afterEach(() => workspace?.dispose());
after(async () => {
  await server?.close();
  delete globalThis.localStorage;
});

test("trigger conflicts compare modifier order and keep an inactive shortcut out of the way", async () => {
  const { triggerConflict } = await server.ssrLoadModule(
    "/src/v3/shared/domain/triggers.ts",
  );
  const p = { ...workspace.state.preferences, hotkey: "Shift + Ctrl + Space" };
  assert.match(triggerConflict(p), /должны отличаться/);
  p.dictationTrigger = "right_ctrl";
  assert.match(triggerConflict(p), /входит в сочетание/);
  p.commandHotkey = "Alt + F9";
  assert.equal(triggerConflict(p), null);
  p.dictationTrigger = "right_alt";
  assert.match(triggerConflict(p), /входит в сочетание/);
  p.dictationTrigger = "x_button2";
  p.hotkey = "unused legacy";
  assert.equal(triggerConflict(p), null);
  p.commandHotkey = "Alt + F4";
  assert.match(triggerConflict(p), /закрывает окно/);
});

test("trigger changes wait for idle; failed saves keep the previous working selection", async () => {
  for (const phase of [
    "listening",
    "silence",
    "transcribing",
    "processing",
    "awaiting_action",
  ]) {
    workspace.state.phase = phase;
    await assert.rejects(
      workspace.settings.save({ dictationTrigger: "x_button1" }),
      /завершите/,
    );
    assert.equal(workspace.state.preferences.dictationTrigger, "shortcut");
  }
  workspace.state.phase = "idle";
  await workspace.settings.save({
    dictationTrigger: "right_alt",
    hotkeyMode: "toggle",
  });
  assert.equal(workspace.state.preferences.dictationTrigger, "right_alt");
  const saved = storage.get("fono-v3-demo-preferences-v1");
  workspace.state.scenario = "save-error";
  await assert.rejects(
    workspace.settings.save({ dictationTrigger: "x_button2" }),
    /Не удалось сохранить/,
  );
  assert.equal(workspace.state.preferences.dictationTrigger, "right_alt");
  assert.equal(storage.get("fono-v3-demo-preferences-v1"), saved);
  workspace.state.scenario = "normal";
  workspace.state.triggerCaptureActive = true;
  await assert.rejects(
    workspace.settings.save({ dictationTrigger: "caps_lock" }),
    /выбор кнопки/,
  );
  workspace.state.triggerCaptureActive = false;
});

test("focused capture waits for release, uses physical key codes and releases its native session", async () => {
  const { createTriggerCapture } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/triggerCapture.ts",
  );
  const previous = globalThis.window;
  const listeners = new Map();
  const calls = [];
  globalThis.window = {
    addEventListener: (name, fn) => listeners.set(name, fn),
    removeEventListener: (name) => listeners.delete(name),
  };
  const emit = (name, fields) =>
    listeners.get(name)?.({
      type: name,
      preventDefault() {},
      stopImmediatePropagation() {},
      ...fields,
    });
  try {
    const results = [];
    const port = createTriggerCapture(workspace.state, {
      begin: async () => "id-1",
      end: async (id) => calls.push(id),
    });
    const session = await port.capture((r) => results.push(r));
    assert.equal(workspace.state.triggerCaptureActive, true);
    emit("keydown", { code: "ControlRight", ctrlKey: true });
    emit("keydown", { code: "KeyA", key: "ф", ctrlKey: true });
    emit("keydown", { code: "KeyA", repeat: true, ctrlKey: true });
    assert.equal(results.length, 0);
    emit("keyup", { code: "KeyA" });
    await session.cancel(); // Must coalesce with the in-flight restoration.
    assert.deepEqual(results, [
      {
        kind: "captured",
        candidate: { trigger: "shortcut", hotkey: "Ctrl + A" },
      },
    ]);
    assert.deepEqual(calls, ["id-1"]);
    assert.equal(workspace.state.triggerCaptureActive, false);
    assert.equal(listeners.size, 0);
    assert.equal(storage.size, 0);
    for (const [name, fields, kind] of [
      ["keyup", { code: "AltRight" }, "captured"],
      ["mouseup", { button: 4 }, "captured"],
      ["blur", {}, "cancelled"],
      ["keydown", { code: "Escape" }, "cancelled"],
      ["keydown", { code: "KeyQ", key: "private" }, "error"],
    ]) {
      const outcomes = [];
      const next = await port.capture((r) => outcomes.push(r));
      if (fields.code === "AltRight") emit("keydown", { code: "AltRight" });
      if (name === "mouseup") emit("mousedown", fields);
      emit(name, fields);
      await next.cancel();
      assert.equal(outcomes[0].kind, kind);
      assert.equal(listeners.size, 0);
      assert.equal(workspace.state.triggerCaptureActive, false);
    }
  } finally {
    globalThis.window = previous;
  }
});

test("capture restoration failure cannot accept a new trigger", async () => {
  const { createTriggerCapture } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/triggerCapture.ts",
  );
  const previous = globalThis.window;
  const listeners = new Map();
  globalThis.window = {
    addEventListener: (n, f) => listeners.set(n, f),
    removeEventListener: (n) => listeners.delete(n),
  };
  try {
    const results = [];
    const port = createTriggerCapture(workspace.state, {
      begin: async () => "id",
      end: async () => {
        throw new Error("private native detail");
      },
    });
    const session = await port.capture((r) => results.push(r));
    await assert.rejects(
      port.capture(() => {}),
      /завершите/,
    );
    listeners.get("mousedown")({
      type: "mousedown",
      button: 3,
      preventDefault() {},
      stopImmediatePropagation() {},
    });
    listeners.get("mouseup")({
      type: "mouseup",
      button: 3,
      preventDefault() {},
      stopImmediatePropagation() {},
    });
    await session.cancel();
    assert.equal(results[0].kind, "error");
    assert.ok(!JSON.stringify(results).includes("private"));
    assert.equal(workspace.state.preferences.dictationTrigger, "shortcut");
    assert.equal(workspace.state.triggerCaptureActive, false);
  } finally {
    globalThis.window = previous;
  }
});

test("trigger UI names the actual button and explains double presses, conflicts and browser limits", async () => {
  const { default: Fields } = await server.ssrLoadModule(
    "/src/v3/features/preferences/presentation/TriggerFields.vue",
  );
  const { workspaceKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/workspace.ts",
  );
  for (const trigger of [
    "shortcut",
    "right_alt",
    "right_ctrl",
    "caps_lock",
    "x_button1",
    "x_button2",
  ]) {
    const app = createSSRApp({
      render: () =>
        h(Fields, {
          modelValue: {
            ...workspace.state.preferences,
            dictationTrigger: trigger,
            hotkeyMode: "toggle",
          },
          active: false,
        }),
    });
    app.provide(workspaceKey, workspace);
    const html = await renderToString(app);
    assert.match(html, /Выбрать кнопку нажатием/);
    assert.match(html, /Педаль и USB-кнопка/);
    assert.match(html, /В браузере проверяется только форма/);
    if (["right_alt", "right_ctrl", "caps_lock"].includes(trigger))
      assert.match(html, /Дважды нажмите кнопку/);
    if (trigger === "right_ctrl")
      assert.match(html, /входит в сочетание команд/);
    if (trigger === "right_alt") assert.match(html, /AltGr/);
  }
});

test("selection preview is explicit demo memory and never enters history, result, storage or clipboard", async () => {
  const history = JSON.stringify(workspace.state.history),
    last = JSON.stringify(workspace.state.last);
  assert.equal(workspace.selection.state.current, null);
  assert.equal(workspace.state.preferences.selectionCaptureEnabled, false);
  assert.equal(workspace.state.preferences.selectionClipboardFallback, false);
  workspace.selection.demo();
  const old = workspace.selection.state.current.operationId;
  assert.equal(workspace.selection.state.current.source, "demo");
  workspace.selection.demo();
  const next = workspace.selection.state.current.operationId;
  await workspace.selection.cancel(old);
  assert.equal(workspace.selection.state.current.operationId, next);
  assert.equal(JSON.stringify(workspace.state.history), history);
  assert.equal(JSON.stringify(workspace.state.last), last);
  assert.equal(storage.size, 0);
  await workspace.selection.cancel(next);
  assert.equal(workspace.selection.state.current, null);
});

test("selection settings roundtrip but cannot overlap a trigger or change during a session", async () => {
  for (const phase of [
    "listening",
    "transcribing",
    "processing",
    "awaiting_action",
  ]) {
    workspace.state.phase = phase;
    await assert.rejects(
      workspace.settings.save({ selectionHotkey: "Ctrl + Alt + E" }),
      /завершите/,
    );
  }
  workspace.state.phase = "idle";
  await workspace.settings.save({
    selectionCaptureEnabled: true,
    selectionClipboardFallback: true,
    selectionHotkey: "Ctrl + Alt + E",
  });
  assert.equal(workspace.state.preferences.selectionHotkey, "Ctrl + Alt + E");
  const saved = JSON.parse(storage.get("fono-v3-demo-preferences-v1"));
  assert.equal(saved.selectionCaptureEnabled, true);
  assert.equal(saved.selectionClipboardFallback, true);
  assert.ok(!JSON.stringify(saved).includes("выделенного фрагмента"));
  await assert.rejects(
    workspace.settings.save({ selectionHotkey: "Ctrl + Shift + Space" }),
    /конфликтует/,
  );
  await assert.rejects(
    workspace.settings.save({ dictationTrigger: "right_alt" }),
    /конфликтует/,
  );
  assert.equal(workspace.state.preferences.dictationTrigger, "shortcut");
});

test("native selection adapter only reads an authorized memory snapshot and cancels its exact identity", async () => {
  const { createNativeSelectionCapture, selectionFromNative } =
    await server.ssrLoadModule(
      "/src/v3/features/selection/infrastructure/nativeSelection.ts",
    );
  const calls = [];
  let notify;
  let released = false;
  let snapshot = {
    operation_id: 7,
    phase: "ready",
    text: "private selected fragment",
    source: "ui_automation",
    target: "must not pass",
    digest: "must not pass",
  };
  const port = createNativeSelectionCapture(
    async (command, args) => {
      calls.push([command, args]);
      if (command === "cancel_selection_capture") snapshot = null;
      return snapshot;
    },
    async (channel, listener) => {
      assert.equal(channel, "selection-capture-changed");
      notify = listener;
      return () => {
        released = true;
      };
    },
  );
  const flush = async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  };
  await flush();
  assert.deepEqual(port.state.current, {
    operationId: 7,
    phase: "ready",
    text: "private selected fragment",
    source: "ui_automation",
  });
  await port.cancel(6);
  assert.equal(calls.length, 1);
  await port.cancel(7);
  assert.deepEqual(calls[1], ["cancel_selection_capture", { operationId: 7 }]);
  assert.equal(port.state.current, null);
  assert.ok(
    calls.every(([name]) =>
      ["get_selection_capture", "cancel_selection_capture"].includes(name),
    ),
  );
  assert.equal(storage.size, 0);
  assert.equal(
    selectionFromNative({
      operation_id: 1,
      phase: "ready",
      text: "",
      source: "clipboard",
    }),
    null,
  );
  assert.equal(
    selectionFromNative({
      operation_id: 1,
      phase: "ready",
      text: "secret",
      source: "unknown",
    }),
    null,
  );
  assert.equal(
    selectionFromNative({
      operation_id: 1,
      phase: "ready",
      text: "x".repeat(32769),
      source: "ui_automation",
    }),
    null,
  );
  assert.ok(
    !selectionFromNative({
      operation_id: 1,
      phase: "error",
      message: "PRIVATE SOURCE PATH",
    }).message.includes("PRIVATE"),
  );
  port.dispose();
  assert.equal(released, true);
  const count = calls.length;
  notify(null);
  await flush();
  assert.equal(calls.length, count);
});

test("selection fields and preview distinguish a demo from real text and offer cancellation", async () => {
  const { default: Preview } = await server.ssrLoadModule(
    "/src/v3/features/selection/presentation/SelectionPreview.vue",
  );
  const { workspaceKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/workspace.ts",
  );
  workspace.selection.demo();
  const app = createSSRApp({ render: () => h(Preview) });
  app.use(WlConfig).use(WlToastService);
  app.provide(workspaceKey, workspace);
  const context = {};
  const html = await renderToString(app, context);
  const all = html + Object.values(context.teleports || {}).join("");
  assert.match(all, /Выделенный текст/);
  assert.match(all, /Демонстрационный фрагмент/);
  assert.match(all, /историю и ИИ он не отправляется/);
  assert.match(all, /Закрыть/);
});

test("last result survives disabled and cleared history; raw archive requires consent", async () => {
  workspace.state.preferences.historyEnabled = false;
  workspace.state.preferences.processingEnabled = false;
  workspace.history.clear();
  workspace.dictation.start();
  await workspace.dictation.finish();
  assert.equal(workspace.state.history.length, 0);
  assert.ok(workspace.state.last.draft.includes("главное"));
  assert.ok(workspace.state.last.entry.original);
  workspace.state.preferences.historyEnabled = true;
  workspace.dictation.start();
  await workspace.dictation.finish();
  assert.equal(workspace.state.history[0].original, undefined);
});

test("manual edit, improvement and undo leave archived text unchanged", async () => {
  const originalArchive = workspace.state.history[0].text;
  workspace.dictation.edit("Ну, проверим интерфейс.");
  await workspace.dictation.improve();
  assert.equal(workspace.state.last.draft, "Проверим интерфейс.");
  workspace.dictation.undoImprove();
  assert.equal(workspace.state.last.draft, "Ну, проверим интерфейс.");
  assert.equal(workspace.state.history[0].text, originalArchive);
});

test("cancelled transcription cannot publish a late result", async () => {
  const before = workspace.state.last.entry.id;
  workspace.dictation.start();
  const completion = workspace.dictation.finish();
  workspace.dictation.cancel();
  await completion;
  assert.equal(workspace.state.phase, "cancelled");
  assert.equal(workspace.state.last.entry.id, before);
});

test("save errors roll back toggles and preserve committed fields", async () => {
  workspace.scenario("save-error");
  await assert.rejects(
    workspace.settings.toggle("serviceEnabled", false),
    /Не удалось сохранить/,
  );
  assert.equal(workspace.state.preferences.serviceEnabled, true);
  assert.equal(workspace.state.pending.serviceEnabled, false);
  await assert.rejects(workspace.settings.save({ language: "en" }));
  assert.equal(workspace.state.preferences.language, "ru");
});

test("pretranscription demo persists only its setting and rolls back when capture wins a save race", async () => {
  assert.equal(workspace.state.preferences.pretranscriptionEnabled, false);
  await workspace.settings.toggle("pretranscriptionEnabled", true);
  const reopened = createWorkspace();
  try {
    assert.equal(reopened.state.preferences.pretranscriptionEnabled, true);
    assert.equal(reopened.state.pretranscription, undefined);
  } finally {
    reopened.dispose();
  }
  workspace.state.scenario = "save-error";
  await assert.rejects(
    workspace.settings.toggle("pretranscriptionEnabled", false),
  );
  assert.equal(workspace.state.preferences.pretranscriptionEnabled, true);
  workspace.state.scenario = "normal";
  const saving = workspace.settings.toggle("pretranscriptionEnabled", false);
  assert.equal(workspace.state.pending.pretranscriptionEnabled, true);
  workspace.dictation.start();
  await assert.rejects(saving, /Сначала завершите/);
  assert.equal(workspace.state.preferences.pretranscriptionEnabled, true);
  assert.equal(workspace.state.pending.pretranscriptionEnabled, false);
  workspace.dictation.cancel();
});

test("mock offline model activation keeps CPU/language capabilities and prevents deleting the selected package", async () => {
  const id = "giga_am:v3_e2e_rnnt_int8";
  const model = workspace.state.models.find((m) => m.id === id);
  assert.ok(model);
  model.status = "installed";
  workspace.state.preferences.acceleration = "vulkan";
  await workspace.settings.save({ model: id, language: "en" });
  assert.equal(workspace.state.preferences.language, "auto");
  assert.equal(workspace.state.preferences.acceleration, "auto");
  assert.throws(() => workspace.settings.removeModel(id), /другую модель/);
  await assert.rejects(
    workspace.settings.save({ language: "en" }),
    /не поддерживает/,
  );
});

test("browser persistence contains preferences but never dictation or instructions", async () => {
  workspace.dictation.edit("PRIVATE TEST TRANSCRIPT");
  await workspace.settings.save({
    language: "en",
    instruction: "PRIVATE TEST PROMPT",
  });
  const serialized = [...storage.values()].join();
  assert.ok(serialized.includes('"language":"en"'));
  assert.ok(!serialized.includes("PRIVATE TEST"));
  const reloaded = createWorkspace();
  assert.equal(reloaded.state.preferences.language, "en");
  assert.equal(reloaded.state.preferences.instruction, "");
  reloaded.dispose();
});

test("demo GPU mode survives reload, rejects invalid modes and does not claim real GPU usage", async () => {
  await workspace.settings.save({ gpuModelResidency: "adaptive" });
  const reloaded = createWorkspace();
  try {
    assert.equal(reloaded.state.preferences.gpuModelResidency, "adaptive");
    assert.equal(reloaded.state.gpuMemory, undefined);
    await assert.rejects(
      reloaded.settings.save({ gpuModelResidency: "per-recording" }),
      /видеопамяти/,
    );
    assert.equal(reloaded.state.preferences.gpuModelResidency, "adaptive");
  } finally {
    reloaded.dispose();
  }
  storage.set(
    "fono-v3-demo-preferences-v1",
    JSON.stringify({ gpuModelResidency: "bad" }),
  );
  const legacy = createWorkspace();
  assert.equal(legacy.state.preferences.gpuModelResidency, "resident");
  legacy.dispose();
});

test("GPU usage reports only available finite measurements", async () => {
  const { gpuMemoryUsage } = await server.ssrLoadModule(
    "/src/v3/shared/domain/gpuMemory.ts",
  );
  assert.equal(gpuMemoryUsage(), null);
  assert.equal(
    gpuMemoryUsage({ monitoring: "unsupported", usedBytes: 5, totalBytes: 10 }),
    null,
  );
  assert.equal(
    gpuMemoryUsage({
      monitoring: "monitoring",
      usedBytes: NaN,
      totalBytes: 10,
    }),
    null,
  );
  assert.equal(
    gpuMemoryUsage({ monitoring: "monitoring", usedBytes: 0, totalBytes: 0 }),
    null,
  );
  assert.equal(
    gpuMemoryUsage({
      monitoring: "monitoring",
      usedBytes: 3 * 1024 ** 3,
      totalBytes: 8 * 1024 ** 3,
    }),
    "3.0 / 8.0 ГБ видеопамяти занято",
  );
});

test("missing device/model and unavailable AI have actionable failures", async () => {
  workspace.scenario("no-microphone");
  workspace.dictation.start();
  assert.equal(workspace.state.phase, "error");
  assert.match(workspace.state.error, /устройство/);
  workspace.scenario("no-model");
  workspace.dictation.start();
  assert.match(workspace.state.error, /Загрузите/);
  workspace.scenario("ai-error");
  const before = workspace.state.last.draft;
  await assert.rejects(workspace.dictation.improve(), /Текст не изменён/);
  assert.equal(workspace.state.last.draft, before);
});

test("trainer requires consent and cloud analysis requires explicit scope permission", async () => {
  await assert.rejects(
    workspace.settings.save({ trainerEnabled: true }),
    /согласие/,
  );
  await workspace.settings.save({
    trainerEnabled: true,
    analyticsConsent: true,
  });
  await assert.rejects(
    workspace.settings.save({
      trainerAiEnabled: true,
      trainerProfile: "cloud",
    }),
    /разрешение/,
  );
  workspace.trainer.clear();
  assert.ok(workspace.state.history.every((entry) => !entry.original));
});

test("queue capacity, cancellation and service state stay consistent", () => {
  workspace.scenario("queue");
  assert.throws(() => workspace.service.enqueue(), /заполнена/);
  workspace.service.cancel("queue-2");
  workspace.service.enqueue();
  assert.equal(
    workspace.state.jobs.filter((j) => ["queued", "running"].includes(j.state))
      .length,
    4,
  );
  workspace.scenario("service-off");
  assert.throws(() => workspace.service.enqueue(), /включите/);
});

test("voice application aliases validate and resolve through the same port", () => {
  workspace.commands.saveApp({
    id: "",
    name: "Редактор",
    phrase: "открой редактор",
    path: "editor.exe",
  });
  assert.match(workspace.commands.test("Открой редактор!"), /Редактор/);
  assert.throws(
    () =>
      workspace.commands.saveApp({
        id: "",
        name: "Другое",
        phrase: "открой редактор",
        path: "other.exe",
      }),
    /занята/,
  );
});

test("history filtering searches raw and final text and respects periods", async () => {
  const { filterHistory } = await server.ssrLoadModule(
    "/src/v3/features/history/application/history.ts",
  );
  assert.equal(
    filterHistory(workspace.state.history, "потом, потом", 7).length,
    1,
  );
  assert.equal(filterHistory(workspace.state.history, "", 7).length, 4);
});

test("saved forms stop being dirty and external toggles stay in sync", async () => {
  const { useDraft } = await server.ssrLoadModule(
    "/src/v3/features/preferences/application/useDraft.ts",
  );
  const { workspaceKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/workspace.ts",
  );
  const { interactionKey, createInteraction } = await server.ssrLoadModule(
    "/src/v3/shared/application/interaction.ts",
  );
  const app = createApp({});
  app.provide(workspaceKey, workspace);
  app.provide(interactionKey, createInteraction());
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: { addEventListener() {}, removeEventListener() {} },
  });
  const scope = effectScope();
  try {
    const form = app.runWithContext(() =>
      scope.run(() => useDraft(["language", "wakeEnabled"])),
    );
    form.draft.language = "en";
    assert.equal(form.dirty.value, true);
    await form.save();
    await nextTick();
    assert.equal(form.dirty.value, false);
    assert.equal(await form.canLeave(), true);
    form.draft.language = "ru";
    const saving = form.save();
    form.draft.language = "auto";
    await saving;
    await nextTick();
    assert.equal(form.draft.language, "auto");
    assert.equal(workspace.state.preferences.language, "ru");
    assert.equal(form.dirty.value, true);
    form.reset();
    await workspace.settings.toggle("wakeEnabled", false);
    await nextTick();
    assert.equal(form.draft.wakeEnabled, false);
    assert.equal(form.dirty.value, false);
  } finally {
    scope.stop();
    delete globalThis.window;
  }
});

test("hidden live pipeline skips AI and publishes one result even when history is disabled", async () => {
  await workspace.settings.save({
    historyEnabled: false,
  });
  // Exercise the retained experimental pipeline without making it user-selectable.
  workspace.state.preferences.dictationMode = "live";
  workspace.state.aiAvailable = false;
  workspace.history.clear();
  workspace.dictation.start();
  assert.equal(workspace.state.live.phase, "listening");
  assert.equal(workspace.state.live.insertionState, "none");
  workspace.state.live.committedText = "Да, да, оставим повторы.";
  workspace.state.live.draftText = "Следующая мысль.";
  await workspace.dictation.finish();
  assert.equal(workspace.state.phase, "done");
  assert.equal(
    workspace.state.last.draft,
    "Да, да, оставим повторы. Следующая мысль.",
  );
  assert.equal(workspace.state.history.length, 0);
  assert.equal(workspace.state.error, "");
  await assert.rejects(workspace.dictation.improve(), /ИИ отключена/);
  workspace.dictation.edit("Правка после завершения");
  assert.equal(workspace.state.last.draft, "Правка после завершения");
});

test("focus pause keeps capture alive and resume is explicit; cancel keeps the confirmed draft", async () => {
  workspace.state.preferences.dictationMode = "live";
  workspace.dictation.start();
  workspace.state.live.insertionState = "paused_focus";
  assert.equal(workspace.state.phase, "listening");
  assert.equal(workspace.state.live.insertionState, "paused_focus");
  assert.throws(() => workspace.dictation.edit("Правка"), /завершите/);
  await assert.rejects(
    workspace.settings.save({ dictationMode: "standard" }),
    /завершите/,
  );
  await workspace.dictation.resumeInsertion();
  assert.equal(workspace.state.live.insertionState, "active");
  const before = workspace.state.history.length;
  workspace.state.live.committedText = "Подтверждённый текст.";
  const available = [
    workspace.state.live.committedText,
    workspace.state.live.draftText,
  ]
    .filter(Boolean)
    .join(" ");
  workspace.dictation.cancel();
  assert.equal(workspace.state.last.draft, available);
  assert.equal(workspace.state.history.length, before);
  assert.equal(workspace.state.live.phase, "cancelled");
});

test("unavailable wake refuses activation and setup while retaining the stored phrase and profile", async () => {
  const phrase = workspace.state.preferences.wakePhrase;
  const profile = JSON.parse(JSON.stringify(workspace.state.wakeSetup));
  assert.equal(workspace.state.preferences.wakeEnabled, false);
  for (const action of [
    () => workspace.settings.toggle("wakeEnabled", true),
    () => workspace.settings.save({ wakeEnabled: true }),
    () => workspace.wake.download(),
    () => workspace.wake.test(),
    () => workspace.wake.begin(),
    () => workspace.wake.record(),
    () => workspace.wake.beginValidation(),
    () => workspace.wake.validate("positive"),
  ])
    await assert.rejects(action(), /Временно недоступно/);
  assert.equal(workspace.state.preferences.wakePhrase, phrase);
  assert.deepEqual(
    JSON.parse(JSON.stringify(workspace.state.wakeSetup)),
    profile,
  );
  for (const scenario of ["normal", "save-error", "no-microphone"]) {
    workspace.scenario(scenario);
    assert.equal(workspace.state.preferences.wakeEnabled, false);
  }
});

test("previous enabled wake preferences normalize off without losing phrase or hotkey mode", async () => {
  storage.set(
    "fono-v3-demo-preferences-v1",
    JSON.stringify({
      wakeEnabled: true,
      wakePhrase: "my helper fono",
      wakeLanguage: "en",
      hotkeyMode: "toggle",
    }),
  );
  const reload = createWorkspace();
  try {
    assert.equal(reload.state.preferences.wakeEnabled, false);
    assert.equal(reload.state.preferences.wakePhrase, "my helper fono");
    assert.equal(reload.state.preferences.hotkeyMode, "toggle");
    await reload.settings.save({ language: "en" });
    assert.equal(
      JSON.parse(storage.get("fono-v3-demo-preferences-v1")).wakeEnabled,
      false,
    );
  } finally {
    reload.dispose();
  }
});

test("wake phrase validation matches native word limits and language alphabets", async () => {
  const { defaults, validatePreferences } = await server.ssrLoadModule(
    "/src/v3/features/preferences/domain/preferences.ts",
  );
  const { normalizeWakePhrase } = await server.ssrLoadModule(
    "/src/v3/features/preferences/domain/wakePhrase.ts",
  );
  assert.equal(defaults.wakePhrase, "Эй, фоно");
  assert.equal(validatePreferences({ ...defaults }), null);
  for (const phrase of [
    "Пожалуйста, активируй помощника",
    "Алё, фоно 2",
    "Компьютер",
    "123",
  ]) {
    assert.equal(
      validatePreferences({ ...defaults, wakePhrase: phrase }),
      null,
      phrase,
    );
  }
  assert.equal(normalizeWakePhrase("  АЛЁ, фоно! 2  "), "але фоно 2");
  assert.match(
    validatePreferences({ ...defaults, wakePhrase: "Эй, Fono" }),
    /кириллицу/,
  );
  assert.match(
    validatePreferences({
      ...defaults,
      wakeLanguage: "en",
      wakePhrase: "hey фоно",
    }),
    /латиницу/,
  );
  assert.equal(
    validatePreferences({
      ...defaults,
      wakeLanguage: "en",
      wakePhrase: "Wake up computer 2",
    }),
    null,
  );
  assert.match(
    validatePreferences({
      ...defaults,
      wakePhrase: "пожалуйста включи запись моего голоса",
    }),
    /от 1 до 4 слов/,
  );
  assert.match(
    validatePreferences({ ...defaults, wakePhrase: "!!!" }),
    /от 1 до 4 слов/,
  );
});

test("invalid mixed-script wake phrase does not change saved demo preferences", async () => {
  await assert.rejects(
    workspace.settings.save({ wakePhrase: "Эй, Fono" }),
    /кириллицу/,
  );
  assert.equal(workspace.state.preferences.wakePhrase, "Эй, фоно");
});

test("previous live demo preferences reload and save as classic without losing other preferences", async () => {
  storage.set(
    "fono-v3-demo-preferences-v1",
    JSON.stringify({
      dictationMode: "live",
      wakePhrase: "my helper fono",
      wakeLanguage: "en",
      processingEnabled: true,
      processingMode: "format",
      transcript: "PRIVATE LIVE TRANSCRIPT",
    }),
  );
  const reload = createWorkspace();
  try {
    assert.equal(reload.state.preferences.dictationMode, "standard");
    assert.equal(reload.state.preferences.wakeLanguage, "en");
    assert.equal(reload.state.preferences.processingEnabled, true);
    assert.equal(reload.state.preferences.processingMode, "format");
    assert.equal(reload.state.live, undefined);
    await reload.settings.save({ dictationMode: "live", overlayScale: 110 });
    assert.equal(reload.state.preferences.dictationMode, "standard");
    const saved = storage.get("fono-v3-demo-preferences-v1");
    assert.equal(JSON.parse(saved).dictationMode, "standard");
    assert.ok(!saved.includes("PRIVATE LIVE TRANSCRIPT"));
    reload.scenario("live-paused");
    assert.equal(reload.state.preferences.dictationMode, "standard");
    assert.equal(reload.state.live, null);
    assert.equal(reload.state.phase, "idle");
  } finally {
    reload.dispose();
  }
});
