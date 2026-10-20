import assert from "node:assert/strict";
import { before, after, beforeEach, afterEach, test } from "node:test";
import { createServer } from "vite";
import { reactive } from "vue";
let server, profiles, mapping, defaults, createWorkspace, workspace, storage;
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
  profiles = await server.ssrLoadModule(
    "/src/v3/shared/domain/applicationProfiles.ts",
  );
  mapping = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/mapping.ts",
  );
  ({ DEFAULT_SETTINGS: defaults } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/ipcTypes.ts",
  ));
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
const configured = () => ({
  id: "ide",
  name: "IDE",
  enabled: true,
  executables: ["code.exe"],
  processingEnabled: true,
  preset: "task",
  translationEnabled: false,
  targetLanguage: null,
  language: "en",
  injectionMode: "sendinput",
});

test("legacy settings have no app rules; native roundtrip deeply owns profile entries without changing global defaults", () => {
  assert.deepEqual(
    mapping.preferencesFromNative(defaults, []).applicationProfiles,
    [],
  );
  const entries = reactive([configured()]);
  const saved = mapping.applyPreferences(
    defaults,
    { applicationProfiles: entries },
    [],
  );
  assert.equal(saved.language, defaults.language);
  assert.equal(saved.injection_mode, defaults.injection_mode);
  const restored = mapping.preferencesFromNative(saved, []);
  assert.deepEqual(restored.applicationProfiles, entries);
  entries[0].executables.push("PRIVATE.exe");
  restored.applicationProfiles[0].name = "other";
  assert.deepEqual(saved.application_profiles[0].executables, ["code.exe"]);
  assert.equal(saved.application_profiles[0].name, "IDE");
});
test("paths, duplicate exe across enabled or disabled profiles, limits and invalid choices are rejected", () => {
  for (const exe of [
    "C:\\PRIVATE\\code.exe",
    "../code.exe",
    "*.exe",
    "code.exe\n",
  ])
    assert.ok(
      profiles.validateApplicationProfiles([
        { ...configured(), executables: [exe] },
      ]),
    );
  assert.ok(
    profiles.validateApplicationProfiles([
      configured(),
      {
        ...configured(),
        id: "other",
        enabled: false,
        executables: ["CODE.EXE"],
      },
    ]),
  );
  assert.ok(
    profiles.validateApplicationProfiles([
      { ...configured(), name: "x".repeat(65) },
    ]),
  );
  assert.ok(
    profiles.validateApplicationProfiles([
      { ...configured(), language: "unsupported" },
    ]),
  );
  assert.ok(
    profiles.validateApplicationProfiles([
      { ...configured(), injectionMode: "shell" },
    ]),
  );
  assert.equal(profiles.validateApplicationProfiles([configured()]), null);
});
test("turning a profile off preserves rules in memory; browser storage never retains configured executable names", async () => {
  const entry = {
    ...configured(),
    name: "PRIVATE PROFILE",
    executables: ["PRIVATE.exe"],
  };
  await workspace.settings.save({ applicationProfiles: [entry] });
  await workspace.settings.save({
    applicationProfiles: [{ ...entry, enabled: false }],
  });
  assert.equal(
    workspace.state.preferences.applicationProfiles[0].enabled,
    false,
  );
  assert.deepEqual(
    workspace.state.preferences.applicationProfiles[0].executables,
    ["PRIVATE.exe"],
  );
  assert.ok(![...storage.values()].join().includes("PRIVATE"));
  assert.equal(workspace.state.preferences.processingMode, "clean");
  const reloaded = createWorkspace();
  try {
    assert.deepEqual(reloaded.state.preferences.applicationProfiles, []);
  } finally {
    reloaded.dispose();
  }
});
