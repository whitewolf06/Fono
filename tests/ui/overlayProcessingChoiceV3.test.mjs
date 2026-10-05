import assert from "node:assert/strict";
import { after, before, beforeEach, test } from "node:test";
import { createServer } from "vite";
import { createApp, effectScope, nextTick, reactive } from "vue";

let server, mapping, browser, preferences, rawDefaults, createQueue, storage;
before(async () => {
  storage = new Map();
  globalThis.localStorage = {
    getItem: (key) => storage.get(key) ?? null,
    setItem: (key, value) => storage.set(key, value),
  };
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { hmr: false, middlewareMode: true },
  });
  mapping = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/mapping.ts",
  );
  browser = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/browser.ts",
  );
  preferences = await server.ssrLoadModule(
    "/src/v3/features/preferences/domain/preferences.ts",
  );
  ({ DEFAULT_SETTINGS: rawDefaults } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/ipcTypes.ts",
  ));
  ({ createOverlayProcessingQueue: createQueue } = await server.ssrLoadModule(
    "/src/v3/features/overlay/infrastructure/overlayProcessingChoice.ts",
  ));
});
beforeEach(() => storage.clear());
after(async () => {
  await server?.close();
  delete globalThis.localStorage;
});
const choice = (preset = "clean", targetLanguage = null) => ({
  preset,
  targetLanguage,
});
const request = (sessionId, preset = "clean", targetLanguage = null) => ({
  sessionId,
  preset,
  targetLanguage,
});
const settings = (preset, targetLanguage) => ({
  ...rawDefaults,
  processing_preset: preset,
  processing_target_language: targetLanguage,
});
const flushMicrotasks = () => new Promise((resolve) => setImmediate(resolve));

test("legacy native settings default quick controls on; explicit off round-trips without losing settings", () => {
  const legacy = structuredClone(rawDefaults);
  delete legacy.overlay_quick_processing;
  assert.equal(
    mapping.preferencesFromNative(legacy, []).overlayQuickProcessing,
    true,
  );
  legacy.overlay_x = -1400;
  legacy.hotkey_mode = "toggle";
  const patched = mapping.applyPreferences(
    legacy,
    { overlayQuickProcessing: false },
    [],
  );
  assert.equal(patched.overlay_quick_processing, false);
  assert.equal(patched.overlay_x, -1400);
  assert.equal(patched.hotkey_mode, "toggle");
  assert.deepEqual(patched.llm_profiles, legacy.llm_profiles);
  assert.equal(
    mapping.preferencesFromNative(patched, []).overlayQuickProcessing,
    false,
  );
  assert.equal(
    mapping.applyPreferences(patched, { language: "en" }, [])
      .overlay_quick_processing,
    false,
  );
});

test("quick controls flag participates in the overlay section and rejects malformed settings", () => {
  assert.equal(preferences.defaults.overlayQuickProcessing, true);
  assert.ok(preferences.sectionKeys.overlay.includes("overlayQuickProcessing"));
  assert.equal(
    preferences.validatePreferences({
      ...preferences.defaults,
      overlayQuickProcessing: false,
    }),
    null,
  );
  assert.match(
    preferences.validatePreferences({
      ...preferences.defaults,
      overlayQuickProcessing: "false",
    }),
    /быстрые настройки/,
  );
});

test("browser persists the optional flag and remembered choice, never instructions or transcripts", () => {
  browser.persistPreferences({
    ...preferences.defaults,
    overlayQuickProcessing: false,
    processingMode: "formal",
    processingTranslation: "en",
    instruction: "PRIVATE PROMPT",
    transcript: "PRIVATE TEXT",
  });
  const raw = [...storage.values()].join();
  assert.ok(!raw.includes("PRIVATE"));
  assert.equal(browser.readPreferences().overlayQuickProcessing, false);
  assert.equal(browser.readPreferences().processingMode, "formal");
  assert.equal(browser.readPreferences().processingTranslation, "en");
  storage.set(
    "fono-v3-demo-preferences-v1",
    JSON.stringify({ overlayQuickProcessing: "false" }),
  );
  assert.equal(browser.readPreferences().overlayQuickProcessing, true);
  storage.set("fono-v3-demo-preferences-v1", "{}");
  assert.equal(browser.readPreferences().overlayQuickProcessing, true);
});

test("settings preview choices stay in the overlay draft until Save; Reset cancels them", async () => {
  const { useDraft } = await server.ssrLoadModule(
    "/src/v3/features/preferences/application/useDraft.ts",
  );
  const { workspaceKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/workspace.ts",
  );
  const { interactionKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/interaction.ts",
  );
  const commits = [];
  const workspace = {
    state: reactive({ preferences: { ...preferences.defaults } }),
    settings: {
      save: async (patch) => {
        commits.push(patch);
        Object.assign(workspace.state.preferences, patch);
        browser.persistPreferences(workspace.state.preferences);
      },
    },
  };
  const app = createApp({});
  app.provide(workspaceKey, workspace);
  app.provide(interactionKey, { confirm: async () => false });
  globalThis.window = { addEventListener() {}, removeEventListener() {} };
  const scope = effectScope();
  try {
    const form = app.runWithContext(() =>
      scope.run(() => useDraft(preferences.sectionKeys.overlay)),
    );
    form.draft.processingMode = "formal";
    form.draft.processingTranslation = "en";
    assert.equal(form.dirty.value, true);
    assert.equal(workspace.state.preferences.processingMode, "clean");
    assert.equal(storage.size, 0);
    form.reset();
    assert.equal(form.draft.processingMode, "clean");
    assert.equal(form.dirty.value, false);
    form.draft.processingMode = "task";
    form.draft.processingTranslation = "de";
    await form.save();
    await nextTick();
    assert.deepEqual(commits, [
      { processingMode: "task", processingTranslation: "de" },
    ]);
    assert.equal(form.dirty.value, false);
    assert.equal(browser.readPreferences().processingTranslation, "de");
  } finally {
    scope.stop();
    delete globalThis.window;
  }
});

test("flush coalesces rapid picks, serializes writes, and waits for the last choice before Stop", async () => {
  const writes = [],
    commits = [],
    order = [];
  let concurrent = 0,
    peak = 0;
  const queue = createQueue(
    {
      readConfirmed: () => choice(),
      isCurrent: (id) => id === 4,
      commit: (s, r) => commits.push([s.processing_preset, r.sessionId]),
      rollback: () => assert.fail("successful save must not roll back"),
      save: (r) => {
        peak = Math.max(peak, ++concurrent);
        order.push("save:" + r.preset);
        return new Promise((resolve) =>
          writes.push({
            r,
            complete: () => {
              concurrent--;
              resolve(settings(r.preset, r.targetLanguage));
            },
          }),
        );
      },
    },
    10000,
  );
  try {
    queue.update(request(4, "task"));
    queue.update(request(4, "formal", "en"));
    const finish = queue.flush().then(() => order.push("stop"));
    assert.equal(writes.length, 1);
    assert.equal(writes[0].r.preset, "formal");
    assert.equal(queue.state.saving, true);
    queue.update(request(4, "format", "de"));
    writes[0].complete();
    await flushMicrotasks();
    assert.equal(writes.length, 2);
    assert.deepEqual(
      commits,
      [],
      "superseded reply does not reset the latest UI choice",
    );
    assert.ok(!order.includes("stop"));
    writes[1].complete();
    await finish;
    assert.deepEqual(commits, [["format", 4]]);
    assert.deepEqual(order, ["save:formal", "save:format", "stop"]);
    assert.equal(peak, 1);
    assert.equal(queue.state.saving, false);
    assert.equal(queue.state.pending, false);
  } finally {
    queue.dispose();
  }
});

test("default quick selection starts persistence immediately without a debounce window", async () => {
  const writes = [];
  const queue = createQueue({
    readConfirmed: () => choice(),
    isCurrent: (id) => id === 1,
    commit: () => {},
    rollback: () => assert.fail("successful choice must not roll back"),
    save: (r) => {
      writes.push(r);
      return Promise.resolve(settings(r.preset, r.targetLanguage));
    },
  });
  try {
    queue.update(request(1, "task", "en"));
    assert.equal(writes.length, 1, "save starts in the same update call");
    await queue.flush();
    assert.equal(queue.state.saving, false);
  } finally {
    queue.dispose();
  }
});

test("a failed latest write restores the last persisted choice and keeps Stop blocked until retry", async () => {
  const writes = [],
    rollback = [];
  const queue = createQueue(
    {
      readConfirmed: () => choice(),
      isCurrent: (id) => id === 8,
      commit: () => {},
      rollback: (error, c) => rollback.push([error.message, c]),
      save: (r) =>
        new Promise((resolve, reject) => writes.push({ r, resolve, reject })),
    },
    10000,
  );
  try {
    queue.update(request(8, "formal", "en"));
    const finishing = queue.flush();
    queue.update(request(8, "task", "ru"));
    writes[0].resolve(settings("formal", "en"));
    await flushMicrotasks();
    writes[1].reject(new Error("Не удалось сохранить"));
    await assert.rejects(finishing, /Не удалось сохранить/);
    assert.deepEqual(rollback, [
      ["Не удалось сохранить", choice("formal", "en")],
    ]);
    await assert.rejects(queue.flush(), /Не удалось сохранить/);
    queue.update(request(8, "task", "ru"));
    const retry = queue.flush();
    writes[2].resolve(settings("task", "ru"));
    await retry;
    assert.equal(queue.state.error, "");
  } finally {
    queue.dispose();
  }
});

test("context change discards queued choices; old replies and errors never update the new session", async () => {
  let context = 2;
  const writes = [],
    commits = [],
    errors = [];
  const queue = createQueue(
    {
      readConfirmed: () => choice(),
      isCurrent: (id) => id === context,
      commit: (_, r) => commits.push(r.sessionId),
      rollback: (error) => errors.push(error.message),
      save: (r) =>
        new Promise((resolve, reject) => writes.push({ r, resolve, reject })),
    },
    10000,
  );
  try {
    queue.update(request(2, "formal"));
    const old = queue.flush();
    queue.update(request(2, "task"));
    context = 3;
    queue.discard();
    queue.update(request(3, "format", "fr"));
    writes[0].reject(new Error("Ошибка старой сессии"));
    await flushMicrotasks();
    assert.equal(writes.length, 2);
    assert.equal(writes[1].r.sessionId, 3);
    writes[1].resolve(settings("format", "fr"));
    await old;
    assert.deepEqual(commits, [3]);
    assert.deepEqual(errors, []);
    queue.update(request(3, "task"));
    queue.dispose();
    await queue.flush();
    assert.equal(
      writes.length,
      2,
      "disposed debounce never starts another write",
    );
  } finally {
    queue.dispose();
  }
});

test("an old successful reply cannot become a new session's rollback value after context switch", async () => {
  let context = 2,
    confirmed = choice("format", "fr");
  const writes = [],
    rollbacks = [],
    commits = [];
  const queue = createQueue(
    {
      readConfirmed: () => confirmed,
      isCurrent: (id) => id === context,
      commit: (_, r) => commits.push(r.sessionId),
      rollback: (_, c) => rollbacks.push(c),
      save: (r) =>
        new Promise((resolve, reject) => writes.push({ r, resolve, reject })),
    },
    10000,
  );
  try {
    queue.update(request(2, "formal", "en"));
    const saving = queue.flush();
    context = 3;
    confirmed = choice("clean", "de");
    queue.discard();
    queue.update(request(3, "task", "ru"));
    writes[0].resolve(settings("formal", "en"));
    await flushMicrotasks();
    writes[1].reject(new Error("Новый выбор не сохранён"));
    await assert.rejects(saving, /не сохранён/);
    assert.deepEqual(commits, []);
    assert.deepEqual(rollbacks, [choice("clean", "de")]);
  } finally {
    queue.dispose();
  }
});
