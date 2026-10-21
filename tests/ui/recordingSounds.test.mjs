import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer } from "vite";

let server,
  preferences,
  mapping,
  browser,
  rawDefaults,
  createWorkspace,
  nativeSettings;
const storage = new Map();
const calls = [];
before(async () => {
  globalThis.localStorage = {
    getItem: (k) => storage.get(k) ?? null,
    setItem: (k, v) => storage.set(k, v),
  };
  globalThis.window = {
    addEventListener() {},
    removeEventListener() {},
    __TAURI_INTERNALS__: {
      invoke: async (command, args) => {
        calls.push({ command, args });
      },
    },
  };
  server = await createServer({
    appType: "custom",
    server: { middlewareMode: true, hmr: false },
  });
  const load = (p) => server.ssrLoadModule("/src/v3/" + p);
  preferences = await load("features/preferences/domain/preferences.ts");
  mapping = await load("shared/infrastructure/native/mapping.ts");
  browser = await load("shared/infrastructure/browser.ts");
  ({ DEFAULT_SETTINGS: rawDefaults } = await load(
    "shared/infrastructure/native/ipcTypes.ts",
  ));
  ({ createMockWorkspace: createWorkspace } = await load(
    "app/mockWorkspace.ts",
  ));
  ({ nativeSettings } = await load("shared/infrastructure/native/settings.ts"));
});
after(async () => {
  await server?.close();
  delete globalThis.localStorage;
  delete globalThis.window;
});

test("legacy settings stay silent; volume round-trips independently of toggle and capture settings", () => {
  const legacy = { ...rawDefaults };
  delete legacy.recording_sounds_enabled;
  delete legacy.recording_sounds_volume;
  const old = mapping.preferencesFromNative(legacy, []);
  assert.equal(old.recordingSoundsEnabled, false);
  assert.equal(old.recordingSoundsVolume, 25);
  for (const volume of [0, 25, 75, 100]) {
    const native = mapping.applyPreferences(
      { ...legacy, language: "en" },
      {
        recordingSoundsEnabled: false,
        recordingSoundsVolume: volume,
      },
      [],
    );
    const next = mapping.preferencesFromNative(native, []);
    assert.equal(next.recordingSoundsEnabled, false);
    assert.equal(next.recordingSoundsVolume, volume);
    assert.equal(native.language, "en");
    assert.equal(
      preferences.validatePreferences({
        ...preferences.defaults,
        recordingSoundsVolume: volume,
      }),
      null,
    );
  }
  for (const value of [-1, 101, 0.5, NaN, Infinity]) {
    assert.ok(
      preferences.validatePreferences({
        ...preferences.defaults,
        recordingSoundsVolume: value,
      }),
    );
  }
});

test("browser reload preserves mute and volume but excludes private data and normalizes malformed volume", () => {
  browser.persistPreferences({
    ...preferences.defaults,
    recordingSoundsEnabled: true,
    recordingSoundsVolume: 75,
    instruction: "PRIVATE INSTRUCTION",
  });
  assert.equal(browser.readPreferences().recordingSoundsEnabled, true);
  assert.equal(browser.readPreferences().recordingSoundsVolume, 75);
  assert.ok(![...storage.values()].join().includes("PRIVATE"));
  for (const volume of [-1, 101, "90", 5.5, null]) {
    storage.set(
      "fono-v3-demo-preferences-v1",
      JSON.stringify({ recordingSoundsVolume: volume }),
    );
    assert.equal(browser.readPreferences().recordingSoundsVolume, 25);
  }
  storage.clear();
});

test("mock save and immediate toggle retain volume; an invalid save leaves confirmed settings intact", async () => {
  const workspace = createWorkspace();
  try {
    await workspace.settings.save({
      recordingSoundsEnabled: true,
      recordingSoundsVolume: 75,
    });
    await workspace.settings.toggle("recordingSoundsEnabled", false);
    assert.equal(workspace.state.preferences.recordingSoundsEnabled, false);
    assert.equal(workspace.state.preferences.recordingSoundsVolume, 75);
    await assert.rejects(
      workspace.settings.save({ recordingSoundsVolume: 200 }),
    );
    assert.equal(workspace.state.preferences.recordingSoundsVolume, 75);
    assert.equal(browser.readPreferences().recordingSoundsEnabled, false);
  } finally {
    workspace.dispose();
    storage.clear();
  }
});

test("explicit preview forwards only draft volume, without saving settings or starting microphone input", async () => {
  calls.length = 0;
  const settings = nativeSettings({
    state: { preferences: { ...preferences.defaults } },
  });
  await settings.previewRecordingSounds(75);
  assert.deepEqual(calls, [
    { command: "preview_recording_sounds", args: { volume: 75 } },
  ]);
});
