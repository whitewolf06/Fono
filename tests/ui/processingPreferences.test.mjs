import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";
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
