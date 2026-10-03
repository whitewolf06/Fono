import assert from "node:assert/strict";
import { before, after, beforeEach, afterEach, test } from "node:test";
import { createServer } from "vite";
import { createApp, effectScope, nextTick } from "vue";
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
    workspace.settings.toggle("wakeEnabled", false),
    /Не удалось сохранить/,
  );
  assert.equal(workspace.state.preferences.wakeEnabled, true);
  assert.equal(workspace.state.pending.wakeEnabled, false);
  await assert.rejects(workspace.settings.save({ language: "en" }));
  assert.equal(workspace.state.preferences.language, "ru");
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

test("live dictation skips AI and publishes one result even when history is disabled", async () => {
  await workspace.settings.save({
    dictationMode: "live",
    historyEnabled: false,
  });
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
  workspace.scenario("live-paused");
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

test("custom wake setup requires five examples, three fresh examples and both negative checks", async () => {
  await workspace.settings.save({
    wakePhrase: "мой помощник фоно",
    wakeLanguage: "ru",
  });
  await workspace.wake.load();
  assert.equal(workspace.state.wakeSetup.verified, false);
  await workspace.wake.begin();
  for (let i = 0; i < 5; i++) await workspace.wake.record();
  assert.equal(workspace.state.wakeSetup.profileReady, true);
  assert.equal(workspace.state.wakeSetup.verified, false);
  await workspace.wake.beginValidation();
  for (let i = 0; i < 3; i++) await workspace.wake.validate("positive");
  await workspace.wake.validate("silence");
  assert.equal(workspace.state.wakeSetup.verified, false);
  await workspace.wake.validate("other_phrase");
  assert.equal(workspace.state.wakeSetup.verified, true);
  assert.equal(workspace.state.wakeSetup.validation.completed, true);
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

test("live mode persists as a demo setting without saving partial text", async () => {
  await workspace.settings.save({
    dictationMode: "live",
    wakePhrase: "my helper fono",
    wakeLanguage: "en",
  });
  workspace.dictation.start();
  workspace.state.live.draftText = "PRIVATE LIVE TRANSCRIPT";
  await workspace.settings.save({ overlayScale: 110 });
  const saved = [...storage.values()].join();
  assert.ok(!saved.includes("PRIVATE LIVE TRANSCRIPT"));
  const reload = createWorkspace();
  assert.equal(reload.state.preferences.dictationMode, "live");
  assert.equal(reload.state.preferences.wakeLanguage, "en");
  assert.equal(reload.state.live, undefined);
  reload.dispose();
});
