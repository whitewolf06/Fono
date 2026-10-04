import assert from "node:assert/strict";
import { after, afterEach, before, beforeEach, test } from "node:test";
import { createServer } from "vite";
import { createSSRApp, effectScope, h } from "vue";
import { renderToString } from "vue/server-renderer";

let server, workspace, port, createWorkspace, createPort, browser, storage;
let useOverlayDemo, overlayScope;
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
    server: { hmr: false, middlewareMode: true },
  });
  ({ createMockWorkspace: createWorkspace } = await server.ssrLoadModule(
    "/src/v3/app/mockWorkspace.ts",
  ));
  ({ createDictationPort: createPort } = await server.ssrLoadModule(
    "/src/v3/features/dictation/infrastructure/mockDictation.ts",
  ));
  browser = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/browser.ts",
  );
  ({ useOverlayDemo } = await server.ssrLoadModule(
    "/src/v3/features/overlay/application/useOverlayDemo.ts",
  ));
});
beforeEach(() => {
  storage.clear();
  workspace = createWorkspace();
  port = createPort(workspace.state, () => Promise.resolve());
  workspace.state.preferences.processingTrigger = "manual";
});
afterEach(() => {
  overlayScope?.stop();
  overlayScope = undefined;
  port?.dispose();
  workspace?.dispose();
});
after(async () => {
  await server?.close();
  delete globalThis.localStorage;
});

async function recorded() {
  port.start();
  await port.finish();
  return workspace.state.pendingDictation;
}

test("manual stop waits for an action and prevents a second capture without archiving", async () => {
  const count = workspace.state.history.length;
  const pending = await recorded();
  assert.equal(workspace.state.phase, "awaiting_action");
  assert.equal(pending.phase, "awaiting_action");
  assert.equal(pending.resultText, null);
  assert.equal(workspace.state.history.length, count);
  assert.equal(workspace.state.last.draft, pending.originalText);
  port.start();
  assert.equal(workspace.state.pendingDictation.sessionId, pending.sessionId);
  assert.equal(workspace.state.phase, "awaiting_action");
});

test("copy-only processing waits for copy completion and blocks insertion actions", async () => {
  const count = workspace.state.history.length;
  const pending = await recorded();
  pending.copyOnly = true;
  await assert.rejects(
    port.resolvePending({ sessionId: pending.sessionId, action: "insert_raw" }),
    /Вставка/,
  );
  await assert.rejects(
    port.resolvePending({
      sessionId: pending.sessionId,
      action: "process_and_insert",
    }),
    /Вставка/,
  );
  await port.resolvePending({
    sessionId: pending.sessionId,
    action: "process_preview",
    preset: "formal",
  });
  assert.equal(workspace.state.phase, "awaiting_action");
  assert.match(workspace.state.pendingDictation.resultText, /Деловое письмо/);
  assert.equal(workspace.state.history.length, count);
  const previewText = workspace.state.pendingDictation.resultText;
  await port.resolvePending({
    sessionId: pending.sessionId,
    action: "complete",
  });
  assert.equal(workspace.state.pendingDictation, null);
  assert.equal(workspace.state.history.length, count + 1);
  assert.equal(workspace.state.history[0].text, previewText);
  await assert.rejects(
    port.resolvePending({ sessionId: pending.sessionId, action: "complete" }),
    /завершена/,
  );
  assert.equal(workspace.state.history.length, count + 1);
});

test("main Copy waits for clipboard success, closes only its copy-only session and never inserts", async () => {
  const { copyPendingText } = await server.ssrLoadModule(
    "/src/v3/features/dictation/application/copyPendingText.ts",
  );
  const first = { sessionId: 14, phase: "awaiting_action", copyOnly: true };
  let current = first;
  const actions = [];
  let release;
  const ports = {
    pending: () => current,
    copy: () =>
      new Promise((resolve) => {
        release = resolve;
      }),
    resolve: async (request) => {
      actions.push(request);
    },
  };
  const copying = copyPendingText(ports, "Ready text.", 14);
  assert.equal(actions.length, 0);
  release();
  await copying;
  assert.deepEqual(actions, [{ sessionId: 14, action: "complete" }]);
  actions.length = 0;
  const staleCopy = copyPendingText(ports, "Old text.", 14);
  current = { sessionId: 15, phase: "awaiting_action", copyOnly: true };
  release();
  await staleCopy;
  assert.equal(actions.length, 0);
  await assert.rejects(
    copyPendingText(
      {
        ...ports,
        copy: async () => {
          throw new Error("Clipboard denied");
        },
      },
      "Text.",
      15,
    ),
    /Clipboard denied/,
  );
  assert.equal(actions.length, 0);
  current.copyOnly = false;
  await copyPendingText({ ...ports, copy: async () => {} }, "Text.", 15);
  assert.equal(actions.length, 0);
});

test("compiled main pending UI offers copy and close for copy-only sessions, with effective disabled translation", async () => {
  const { default: PendingActions } = await server.ssrLoadModule(
    "/src/v3/shared/presentation/PendingDictationActions.vue",
  );
  const html = await renderToString(
    createSSRApp({
      render: () =>
        h(PendingActions, {
          pending: {
            sessionId: 8,
            phase: "awaiting_action",
            copyOnly: true,
            originalText: "Raw text.",
            resultText: "Ready text.",
            preset: "clean",
            targetLanguage: "en",
            processingEnabled: true,
            translationEnabled: false,
            error: null,
            insertionBlocked: false,
            source: "hotkey",
          },
        }),
    }),
  );
  assert.doesNotMatch(html, /Вставить исходный текст|>\s*Исходный\s*</);
  assert.match(html, /Копировать текст и закрыть индикатор/);
  assert.match(html, /Закрыть без копирования/);
  assert.doesNotMatch(html, /Обработать · EN/);
  assert.match(html, /aria-pressed="true" aria-label="Без перевода"/);
  assert.match(html, /aria-pressed="false" aria-label="Английский"/);
});

test("insertion failure has one notice, no listening wave, and usable copy and close actions", async () => {
  const pending = await recorded();
  const reason =
    "ошибка вставки текста: Поле для вставки изменилось или недоступно.";
  pending.error = reason;
  pending.insertionBlocked = true;
  workspace.state.error = reason;
  const { default: RecordControl } = await server.ssrLoadModule(
    "/src/v3/features/dictation/presentation/RecordControl.vue",
  );
  const { workspaceKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/workspace.ts",
  );
  const { WlToastService } = await import("@whitelife-core/ui-kit");
  const app = createSSRApp({ render: () => h(RecordControl) });
  app.provide(workspaceKey, workspace);
  app.use(WlToastService);
  const html = await renderToString(app);
  assert.equal(html.split(reason).length - 1, 1);
  assert.doesNotMatch(html, /voice-wave|record-error|Вставить исходный текст/);
  assert.match(html, /Вставка остановлена/);
  assert.match(html, /часть текста могла уже вставиться/);
  assert.match(html, /<details[^>]*><summary>Подробнее<\/summary>/);
  assert.doesNotMatch(html, /<details[^>]*\bopen\b/);
  assert.match(html, /aria-label="Копировать текст"/);
  assert.match(html, /Закрыть без копирования/);
});

test("a clipboard failure stays visible even when insertion is blocked", async () => {
  const { dictationNotice } = await server.ssrLoadModule(
    "/src/v3/shared/domain/dictationNotice.ts",
  );
  const notice = dictationNotice("Не удалось скопировать текст.", {
    insertionBlocked: true,
    hasText: true,
  });
  assert.equal(notice.kind, "copy");
  assert.equal(notice.title, "Не удалось скопировать");
  assert.equal(dictationNotice(""), null);
  assert.equal(
    dictationNotice("", { insertionBlocked: true }).kind,
    "insertion",
  );
});

test("overlay uses a short insertion status and does not offer processing retry", async () => {
  const { default: OverlayPreview } = await server.ssrLoadModule(
    "/src/v3/features/overlay/presentation/OverlayPreview.vue",
  );
  const pending = await recorded();
  pending.insertionBlocked = true;
  pending.error = "ошибка вставки текста: Поле для вставки изменилось.";
  const html = await renderToString(
    createSSRApp({
      render: () =>
        h(OverlayPreview, {
          preferences: workspace.state.preferences,
          pending,
        }),
    }),
  );
  assert.match(html, /Вставка остановлена/);
  assert.match(html, /Скопировать текст и закрыть/);
  assert.match(html, /Подробнее/);
  assert.doesNotMatch(html, /Повторить обработку без вставки|voice-wave/);
  assert.equal(html.split(pending.error).length - 1, 1);
});

test("manual setting does not hold a dictation when processing is disabled", async () => {
  workspace.state.preferences.processingEnabled = false;
  const count = workspace.state.history.length;
  await recorded();
  assert.equal(workspace.state.pendingDictation, null);
  assert.equal(workspace.state.phase, "done");
  assert.equal(workspace.state.history.length, count + 1);
  assert.equal(workspace.state.last.entry.metadata.processingMode, "off");
});

test("raw insertion archives exactly once and a repeated old action is rejected", async () => {
  const count = workspace.state.history.length;
  const pending = await recorded();
  await port.resolvePending({
    sessionId: pending.sessionId,
    action: "insert_raw",
  });
  assert.equal(workspace.state.phase, "done");
  assert.equal(workspace.state.pendingDictation, null);
  assert.equal(workspace.state.history.length, count + 1);
  assert.equal(workspace.state.last.draft, pending.originalText);
  await assert.rejects(
    port.resolvePending({ sessionId: pending.sessionId, action: "insert_raw" }),
    /завершена или отменена/,
  );
  assert.equal(workspace.state.history.length, count + 1);
});

test("preset and translation actions produce labeled demo results without overwriting raw text", async () => {
  const pending = await recorded();
  await port.resolvePending({
    sessionId: pending.sessionId,
    action: "process_and_insert",
    preset: "formal",
    targetLanguage: "en",
  });
  assert.match(workspace.state.last.draft, /Демонстрационный перевод · EN/);
  assert.match(workspace.state.last.draft, /Деловое письмо \(демо\)/);
  assert.equal(workspace.state.last.entry.original, pending.originalText);
  assert.equal(workspace.state.last.variant, "result");
  assert.equal(workspace.state.last.entry.metadata.processingMode, "formal");
  port.chooseVariant("original");
  assert.equal(workspace.state.last.draft, pending.originalText);
});

test("unavailable AI preserves pending dictation and one retry archives once", async () => {
  const count = workspace.state.history.length;
  workspace.state.aiAvailable = false;
  const pending = await recorded();
  await port.resolvePending({
    sessionId: pending.sessionId,
    action: "process_and_insert",
  });
  assert.equal(workspace.state.phase, "awaiting_action");
  assert.equal(workspace.state.pendingDictation.sessionId, pending.sessionId);
  assert.match(workspace.state.pendingDictation.error, /недоступна/);
  assert.equal(workspace.state.history.length, count);
  workspace.state.aiAvailable = true;
  await Promise.all([
    port.resolvePending({
      sessionId: pending.sessionId,
      action: "process_and_insert",
      preset: "task",
    }),
    port.resolvePending({
      sessionId: pending.sessionId,
      action: "process_and_insert",
      preset: "task",
    }),
  ]);
  assert.equal(workspace.state.history.length, count + 1);
  assert.match(workspace.state.last.draft, /Задача \(демо\)/);
});

test("automatic AI failure also waits for explicit raw insertion or retry", async () => {
  workspace.state.preferences.processingTrigger = "automatic";
  workspace.state.aiAvailable = false;
  const count = workspace.state.history.length;
  const pending = await recorded();
  assert.equal(workspace.state.phase, "awaiting_action");
  assert.equal(workspace.state.history.length, count);
  await port.resolvePending({
    sessionId: pending.sessionId,
    action: "insert_raw",
  });
  assert.equal(workspace.state.phase, "done");
  assert.equal(workspace.state.history.length, count + 1);
  assert.equal(workspace.state.error, "");
});

test("cancellation while processing fences late results and stale actions from the next capture", async () => {
  let hold = false;
  let release;
  port.dispose();
  port = createPort(workspace.state, () =>
    hold
      ? new Promise((resolve) => {
          release = resolve;
        })
      : Promise.resolve(),
  );
  const first = await recorded();
  const count = workspace.state.history.length;
  hold = true;
  const processing = port.resolvePending({
    sessionId: first.sessionId,
    action: "process_and_insert",
  });
  assert.equal(workspace.state.phase, "processing");
  port.cancel();
  hold = false;
  const next = await recorded();
  assert.notEqual(next.sessionId, first.sessionId);
  release();
  await processing;
  assert.equal(workspace.state.pendingDictation.sessionId, next.sessionId);
  assert.equal(workspace.state.phase, "awaiting_action");
  assert.equal(workspace.state.history.length, count);
  await assert.rejects(
    port.resolvePending({ sessionId: first.sessionId, action: "cancel" }),
    /завершена или отменена/,
  );
  assert.equal(workspace.state.pendingDictation.sessionId, next.sessionId);
});

test("canceling a waiting dictation does not archive it", async () => {
  const count = workspace.state.history.length;
  const pending = await recorded();
  await port.resolvePending({ sessionId: pending.sessionId, action: "cancel" });
  assert.equal(workspace.state.pendingDictation, null);
  assert.equal(workspace.state.phase, "cancelled");
  assert.equal(workspace.state.history.length, count);
});

test("capture snapshot retains processing selection, metadata, privacy and dictionary choices", async () => {
  const preferences = workspace.state.preferences;
  preferences.historyEnabled = false;
  preferences.dictionaryEnabled = true;
  preferences.dictionaryEntries = [
    { written: "Fono V3", spoken: ["интерфейс"] },
  ];
  preferences.processingMode = "format";
  preferences.processingTranslation = "en";
  preferences.language = "ru";
  const count = workspace.state.history.length;
  port.start();
  preferences.historyEnabled = true;
  preferences.dictionaryEnabled = false;
  preferences.dictionaryEntries = [];
  preferences.processingMode = "clean";
  preferences.processingTranslation = "none";
  preferences.processingTrigger = "automatic";
  preferences.language = "en";
  await port.finish();
  const pending = workspace.state.pendingDictation;
  assert.equal(pending.preset, "format");
  assert.equal(pending.targetLanguage, "en");
  await port.resolvePending({
    sessionId: pending.sessionId,
    action: "insert_raw",
  });
  assert.equal(workspace.state.history.length, count);
  assert.match(workspace.state.last.draft, /Fono V3/);
  assert.ok(!workspace.state.last.entry.original.includes("Fono V3"));
  assert.equal(workspace.state.last.entry.metadata.language, "ru");
  assert.equal(workspace.state.last.entry.metadata.dictionaryEnabled, true);
});

test("new preferences persist without pending text or instructions", async () => {
  workspace.state.preferences.hotkeyMode = "toggle";
  workspace.state.preferences.processingMode = "task";
  workspace.state.preferences.processingTranslation = "de";
  workspace.state.preferences.instruction = "PRIVATE TEST PROMPT";
  const pending = await recorded();
  browser.persistPreferences(workspace.state.preferences);
  const saved = [...storage.values()].join();
  const restored = browser.readPreferences();
  assert.equal(restored.hotkeyMode, "toggle");
  assert.equal(restored.processingTrigger, "manual");
  assert.equal(restored.processingMode, "task");
  assert.equal(restored.processingTranslation, "de");
  assert.ok(!saved.includes(pending.originalText));
  assert.ok(!saved.includes("PRIVATE TEST PROMPT"));
});

test("invalid persisted modes fall back to supported defaults", () => {
  storage.set(
    "fono-v3-demo-preferences-v1",
    JSON.stringify({
      hotkeyMode: "invalid",
      processingTrigger: "invalid",
      processingMode: "invalid",
      processingTranslation: "invalid",
    }),
  );
  const restored = browser.readPreferences();
  assert.equal(restored.hotkeyMode, "hold");
  assert.equal(restored.processingTrigger, "automatic");
  assert.equal(restored.processingMode, "clean");
  assert.equal(restored.processingTranslation, "none");
});

function overlayDemo() {
  overlayScope = effectScope();
  return overlayScope.run(() =>
    useOverlayDemo(() => workspace.state.preferences),
  );
}

test("overlay demo finishes capture to a local pending action, then processes the selected preset", async () => {
  const demo = overlayDemo();
  const finishing = demo.finish();
  assert.equal(demo.phase.value, "transcribing");
  await finishing;
  const pending = demo.pending.value;
  assert.equal(demo.phase.value, "awaiting_action");
  const processing = demo.resolve({
    sessionId: pending.sessionId,
    action: "process_and_insert",
    preset: "task",
    targetLanguage: "en",
  });
  assert.equal(demo.phase.value, "processing");
  await processing;
  assert.equal(demo.phase.value, "done");
  assert.equal(demo.pending.value, null);
  assert.match(demo.resultText.value, /Демонстрационный перевод · EN/);
  assert.match(demo.resultText.value, /Задача \(демо\)/);
  assert.equal(workspace.state.pendingDictation, undefined);
  assert.equal(workspace.state.phase, "idle");
});

test("overlay demo cancels processing and ignores late actions from its previous local session", async () => {
  const demo = overlayDemo();
  demo.select("awaiting_action");
  const first = demo.pending.value.sessionId;
  const processing = demo.resolve({
    sessionId: first,
    action: "process_and_insert",
  });
  demo.cancel();
  demo.select("awaiting_action");
  const next = demo.pending.value.sessionId;
  await processing;
  await demo.resolve({ sessionId: first, action: "insert_raw" });
  assert.equal(demo.phase.value, "awaiting_action");
  assert.equal(demo.pending.value.sessionId, next);
  assert.equal(demo.resultText.value, "");
  const raw = demo.pending.value.originalText;
  await demo.resolve({ sessionId: next, action: "insert_raw" });
  assert.equal(demo.phase.value, "done");
  assert.equal(demo.resultText.value, raw);
});

test("overlay pending error preserves choices and permits local retry", async () => {
  workspace.state.preferences.processingMode = "formal";
  workspace.state.preferences.processingTranslation = "fr";
  const demo = overlayDemo();
  demo.showPendingError();
  const pending = demo.pending.value;
  assert.match(pending.error, /Пример ошибки/);
  assert.equal(pending.preset, "formal");
  assert.equal(pending.targetLanguage, "fr");
  await demo.resolve({
    sessionId: pending.sessionId,
    action: "process_and_insert",
  });
  assert.equal(demo.phase.value, "done");
  assert.match(demo.resultText.value, /Демонстрационный перевод · FR/);
  assert.match(demo.resultText.value, /Деловое письмо \(демо\)/);
});
