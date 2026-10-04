import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";
import { createApp, effectScope, nextTick, reactive, ref } from "vue";
let server,
  defaults,
  quickKeys,
  sectionKeys,
  validate,
  toNative,
  fromNative,
  settings,
  metadata,
  facts;
before(async () => {
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { hmr: false, middlewareMode: true },
  });
  const p = await server.ssrLoadModule(
    "/src/v3/features/preferences/domain/preferences.ts",
  );
  ({ defaults, quickKeys, sectionKeys, validatePreferences: validate } = p);
  ({ applyPreferences: toNative, preferencesFromNative: fromNative } =
    await server.ssrLoadModule(
      "/src/v3/shared/infrastructure/native/mapping.ts",
    ));
  ({ DEFAULT_SETTINGS: settings } =
    await server.ssrLoadModule("/src/lib/types.ts"));
  ({ historyMetadataFromNative: metadata } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/historyMetadata.ts",
  ));
  ({ dictationFacts: facts } = await server.ssrLoadModule(
    "/src/v3/features/history/application/statistics.ts",
  ));
});
after(async () => await server?.close());
test("legacy settings preserve format, automatic workflow, hold shortcut, no translation", () => {
  const old = { ...settings, ai_mode: "format" };
  delete old.hotkey_mode;
  delete old.processing_workflow;
  delete old.processing_preset;
  delete old.processing_target_language;
  const p = fromNative(old, []);
  assert.equal(p.hotkeyMode, "hold");
  assert.equal(p.processingMode, "format");
  assert.equal(p.processingTrigger, "automatic");
  assert.equal(p.processingTranslation, "none");
});
test("new settings roundtrip through native fields and retain disabled defaults", () => {
  const patch = {
    hotkeyMode: "toggle",
    processingEnabled: true,
    processingMode: "task",
    processingTrigger: "manual",
    processingTranslation: "en",
  };
  const native = toNative(settings, patch, []);
  const p = fromNative(native, []);
  for (const [key, value] of Object.entries(patch)) assert.equal(p[key], value);
  const off = toNative(native, { processingEnabled: false }, []);
  assert.equal(off.ai_mode, "off");
  assert.equal(fromNative(off, []).processingMode, "task");
  assert.equal(fromNative(off, []).processingTranslation, "en");
  const clearTranslation = toNative(
    native,
    { processingTranslation: "none", processingMode: "formal" },
    [],
  );
  assert.equal(clearTranslation.processing_target_language, null);
  assert.equal(fromNative(clearTranslation, []).processingMode, "formal");
});
test("quick forms and complete sections save all new fields", () => {
  assert.ok(quickKeys.hotkey.includes("hotkeyMode"));
  assert.ok(sectionKeys.activation.includes("hotkeyMode"));
  for (const key of [
    "processingTrigger",
    "processingMode",
    "processingTranslation",
  ]) {
    assert.ok(quickKeys.processing.includes(key));
    assert.ok(sectionKeys.processing.includes(key));
  }
});
test("invalid enum preferences are rejected with useful messages", () => {
  for (const key of [
    "hotkeyMode",
    "processingTrigger",
    "processingMode",
    "processingTranslation",
  ]) {
    assert.ok(validate({ ...defaults, [key]: "invalid" }));
  }
});
test("history displays actual selected preset and translation, raw bypass stays off", () => {
  const m = metadata({
    metadata: {
      processing_mode: "clean",
      processing_preset: "task",
      processing_workflow: "manual",
      processing_target_language: "en",
    },
  });
  const f = facts({ duration: 1, metadata: m });
  assert.ok(f.some((x) => x.value === "Постановка задачи"));
  assert.ok(
    f.some((x) => x.title === "Перевод результата" && x.value === "Английский"),
  );
  assert.equal(
    metadata({
      metadata: {
        processing_mode: "off",
        processing_preset: null,
        processing_target_language: null,
      },
    }).processingMode,
    "off",
  );
});

test("per-style prompts migrate legacy clean instruction and roundtrip independently", () => {
  const legacy = fromNative(
    { ...settings, clean_prompt: "Keep my wording." },
    [],
  );
  assert.deepEqual(legacy.processingPrompts.clean, {
    useCustom: true,
    customPrompt: "Keep my wording.",
  });
  assert.equal(legacy.processingPrompts.formal.useCustom, false);
  const custom = structuredClone(legacy.processingPrompts);
  custom.task = {
    useCustom: true,
    customPrompt: "Turn this into a task, do not solve it.",
  };
  const wire = toNative(
    settings,
    {
      processingPrompts: custom,
      processingMode: "raw",
      processingTranslation: "en",
      processingTranslationEnabled: false,
    },
    [],
  );
  assert.equal(wire.clean_prompt, null);
  assert.deepEqual(fromNative(wire, []).processingPrompts, custom);
  assert.equal(fromNative(wire, []).processingMode, "raw");
  assert.equal(fromNative(wire, []).processingTranslation, "en");
  assert.equal(fromNative(wire, []).processingTranslationEnabled, false);
});

test("nested prompt drafts stay isolated, reset fully and preserve edits during save", async () => {
  const { useDraft } = await server.ssrLoadModule(
    "/src/v3/features/preferences/application/useDraft.ts",
  );
  const { workspaceKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/workspace.ts",
  );
  const { interactionKey, createInteraction } = await server.ssrLoadModule(
    "/src/v3/shared/application/interaction.ts",
  );
  const state = reactive({ preferences: structuredClone(defaults) });
  let resolveSave;
  const workspace = {
    state,
    settings: {
      save: (patch) =>
        new Promise((resolve) => {
          resolveSave = () => {
            Object.assign(state.preferences, structuredClone(patch));
            resolve();
          };
        }),
    },
  };
  const app = createApp({});
  app.provide(workspaceKey, workspace);
  app.provide(interactionKey, createInteraction());
  globalThis.window = { addEventListener() {}, removeEventListener() {} };
  const scope = effectScope();
  try {
    const form = app.runWithContext(() =>
      scope.run(() => useDraft(["processingPrompts"])),
    );
    form.draft.processingPrompts.clean.customPrompt = "Private draft.";
    form.draft.processingPrompts.clean.useCustom = true;
    assert.equal(form.dirty.value, true);
    assert.equal(state.preferences.processingPrompts.clean.customPrompt, "");
    form.reset();
    assert.equal(form.dirty.value, false);
    form.draft.processingPrompts.formal = {
      useCustom: true,
      customPrompt: "Saved instruction.",
    };
    const pending = form.save();
    form.draft.processingPrompts.formal.customPrompt = "Newer unsaved edit.";
    resolveSave();
    await pending;
    await nextTick();
    assert.equal(
      state.preferences.processingPrompts.formal.customPrompt,
      "Saved instruction.",
    );
    assert.equal(
      form.draft.processingPrompts.formal.customPrompt,
      "Newer unsaved edit.",
    );
    assert.equal(form.dirty.value, true);
    form.reset();
    assert.equal(
      form.draft.processingPrompts.formal.customPrompt,
      "Saved instruction.",
    );
  } finally {
    scope.stop();
    delete globalThis.window;
  }
});

test("native preview translates prompt overrides and owns capture cancellation across delayed start", async () => {
  const { nativeProcessing } = await server.ssrLoadModule(
    "/src/v3/features/preferences/infrastructure/nativeProcessing.ts",
  );
  const calls = [];
  let resolveStart;
  globalThis.window = {
    __TAURI_INTERNALS__: {
      invoke: async (command, args) => {
        calls.push([command, args]);
        if (command === "start_processing_test_capture")
          return new Promise((resolve) => {
            resolveStart = resolve;
          });
        if (command === "preview_processing_text")
          return {
            text: "Edited text.",
            model: "test-model",
            elapsed_ms: 22,
            preset: "task",
            target_language: null,
          };
      },
    },
  };
  try {
    const port = nativeProcessing();
    const result = await port.previewProcessing({
      text: "Source?",
      preset: "task",
      targetLanguage: null,
      promptOverride: { useCustom: true, customPrompt: "Edit only." },
    });
    assert.equal(result.elapsedMs, 22);
    assert.deepEqual(calls[0][1].input.promptOverride, {
      use_custom: true,
      custom_prompt: "Edit only.",
    });
    const start = port.startProcessingCapture();
    const cancelled = port.cancelProcessingCapture();
    resolveStart({ sessionId: 41 });
    await start;
    await cancelled;
    assert.deepEqual(calls.at(-1), [
      "cancel_processing_test_capture",
      { sessionId: 41 },
    ]);
    const count = calls.length;
    await port.cancelProcessingCapture();
    assert.equal(calls.length, count);
  } finally {
    delete globalThis.window;
  }
});

test("browser raw preview and test dictation do not publish last text or archive", async () => {
  const { mockProcessing } = await server.ssrLoadModule(
    "/src/v3/features/preferences/infrastructure/mockProcessing.ts",
  );
  const state = {
    preferences: structuredClone(defaults),
    last: { draft: "Existing last text." },
    history: [{ text: "Archive." }],
    aiAvailable: false,
    microphoneAvailable: true,
  };
  const port = mockProcessing(state);
  const before = structuredClone(state);
  const result = await port.previewProcessing({
    text: "Raw question?",
    preset: "raw",
    targetLanguage: null,
  });
  assert.equal(result.text, "Raw question?");
  assert.equal(result.model, null);
  await port.startProcessingCapture();
  assert.ok(await port.finishProcessingCapture());
  assert.deepEqual(state, before);
});

test("processing preview preserves typed input after silent capture and checks draft prompt before save", async () => {
  const { useProcessingPreview } = await server.ssrLoadModule(
    "/src/v3/features/preferences/application/useProcessingPreview.ts",
  );
  const { workspaceKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/workspace.ts",
  );
  const state = reactive({
    preferences: structuredClone(defaults),
    phase: "idle",
    last: {
      entry: { original: "Archived raw text." },
      draft: "Last processed text.",
    },
  });
  const calls = [];
  const workspace = {
    state,
    settings: {
      async startProcessingCapture() {},
      async finishProcessingCapture() {
        return "";
      },
      async cancelProcessingCapture() {},
      async previewProcessing(input) {
        calls.push(input);
        return {
          text: "Edited question?",
          model: "test-model",
          elapsedMs: 12,
          preset: input.preset,
          targetLanguage: input.targetLanguage,
        };
      },
    },
    copy: async () => {},
  };
  const app = createApp({});
  app.provide(workspaceKey, workspace);
  const scope = effectScope();
  try {
    const draft = ref(structuredClone(defaults));
    const form = app.runWithContext(() =>
      scope.run(() => useProcessingPreview(draft)),
    );
    form.input.value = "Typed question?";
    await form.startCapture();
    await form.finishCapture();
    assert.equal(form.input.value, "Typed question?");
    assert.match(form.notice.value, /Речь не обнаружена/);
    draft.value.processingPrompts.clean = {
      useCustom: true,
      customPrompt: "Preserve questions.",
    };
    await form.test();
    assert.deepEqual(calls[0].promptOverride, {
      useCustom: true,
      customPrompt: "Preserve questions.",
    });
    assert.equal(state.preferences.processingPrompts.clean.useCustom, false);
    assert.equal(state.last.draft, "Last processed text.");
    draft.value.profile = "unsaved-profile";
    assert.equal(form.connectionChanged.value, true);
    assert.equal(form.canTest.value, false);
    draft.value.processingMode = "raw";
    assert.equal(form.usesModel.value, false);
    assert.equal(form.canTest.value, true);
  } finally {
    scope.stop();
  }
});
