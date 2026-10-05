import assert from "node:assert/strict";
import { after, before, beforeEach, afterEach, test } from "node:test";
import { createServer } from "vite";
import { reactive } from "vue";
let server,
  dictionary,
  createWorkspace,
  workspace,
  storage,
  mapping,
  rawDefaults;
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
  dictionary = await server.ssrLoadModule(
    "/src/v3/shared/domain/personalDictionary.ts",
  );
  ({ createMockWorkspace: createWorkspace } = await server.ssrLoadModule(
    "/src/v3/app/mockWorkspace.ts",
  ));
  mapping = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/mapping.ts",
  );
  ({ DEFAULT_SETTINGS: rawDefaults } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/ipcTypes.ts",
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
const configured = (dictionaryEntries) => ({
  dictionaryEnabled: true,
  dictionaryEntries,
});

test("dictionary uses whole Unicode words, longest phrase and a single non-cascading pass", () => {
  const preferences = configured([
    { written: "Fono", spoken: ["фоно", "fono"] },
    { written: "White", spoken: ["вайт"] },
    { written: "WhiteLife", spoken: ["вайт лайф"] },
    { written: "Other", spoken: ["WhiteLife"] },
  ]);
  assert.equal(
    dictionary.canonicalizeDictionary(
      preferences,
      "ФОНО, fono! микрофоно fonometer _fono fono2 fono\u0301",
    ),
    "Fono, Fono! микрофоно fonometer _fono fono2 fono\u0301",
  );
  assert.equal(
    dictionary.canonicalizeDictionary(preferences, "ВАЙТ\t лайф и вайт"),
    "WhiteLife и White",
  );
  assert.equal(
    dictionary.canonicalizeDictionary(preferences, "вайт, лайф"),
    "White, лайф",
  );
  assert.equal(
    dictionary.canonicalizeDictionary(
      { ...preferences, dictionaryEnabled: false },
      "фоно",
    ),
    "фоно",
  );
});

test("validation rejects ambiguous phrases, malformed input and size overruns", () => {
  const bad = [
    null,
    [{ written: "Fono", spoken: "фоно" }],
    [{ written: "Fono", spoken: ["фоно", "ФОНО"] }],
    [
      { written: "A", spoken: ["same"] },
      { written: "B", spoken: ["SAME"] },
    ],
    [{ written: "A", spoken: ["..."] }],
    [{ written: "A", spoken: ["\nфоно"] }],
    [{ written: "я".repeat(121), spoken: ["фоно"] }],
    Array.from({ length: 129 }, (_, i) => ({
      written: `A${i}`,
      spoken: [`B${i}`],
    })),
  ];
  for (const entries of bad) assert.ok(dictionary.validateDictionary(entries));
  const ambiguous = configured(bad[3]);
  assert.equal(dictionary.canonicalizeDictionary(ambiguous, "same"), "same");
});

test("browser never persists dictionary terms and turning it off keeps current entries", async () => {
  const entries = [{ written: "PRIVATE PROJECT", spoken: ["private phrase"] }];
  assert.equal(workspace.state.preferences.dictionaryEnabled, false);
  await workspace.settings.save({
    dictionaryEnabled: true,
    dictionaryEntries: entries,
  });
  await workspace.settings.toggle("dictionaryEnabled", false);
  assert.deepEqual(workspace.state.preferences.dictionaryEntries, entries);
  const serialized = [...storage.values()].join();
  assert.ok(!serialized.includes("PRIVATE"));
  assert.ok(!serialized.includes("private phrase"));
  assert.ok(!serialized.includes("dictionaryEntries"));
  const reloaded = createWorkspace();
  assert.equal(reloaded.state.preferences.dictionaryEnabled, false);
  assert.deepEqual(reloaded.state.preferences.dictionaryEntries, []);
  reloaded.dispose();
});

test("native preferences roundtrip reactive draft entries and disabling does not remove them", () => {
  const entries = [{ written: "Fono", spoken: ["фоно"] }];
  const enabled = mapping.applyPreferences(
    rawDefaults,
    { dictionaryEnabled: true, dictionaryEntries: reactive(entries) },
    [],
  );
  const disabled = mapping.applyPreferences(
    enabled,
    { dictionaryEnabled: false },
    [],
  );
  assert.deepEqual(disabled.personal_dictionary_entries, entries);
  assert.equal(disabled.personal_dictionary_enabled, false);
  assert.deepEqual(
    mapping.preferencesFromNative(disabled, []).dictionaryEntries,
    entries,
  );
  assert.deepEqual(rawDefaults.personal_dictionary_entries, []);
});

test("ordinary result and archive use dictionary spelling while original stays untouched", async () => {
  await workspace.settings.save({
    processingEnabled: false,
    dictionaryEnabled: true,
    dictionaryEntries: [{ written: "Fono", spoken: ["новый интерфейс"] }],
    analyticsConsent: true,
    trainerEnabled: true,
  });
  workspace.dictation.start();
  await workspace.dictation.finish();
  assert.ok(workspace.state.last.entry.text.includes("Fono"));
  assert.ok(workspace.state.last.entry.original.includes("новый интерфейс"));
  assert.ok(workspace.state.history[0].text.includes("Fono"));
  assert.ok(workspace.state.history[0].original.includes("новый интерфейс"));
  assert.equal(workspace.state.last.variant, "result");
  assert.equal(workspace.state.last.draft, workspace.state.last.entry.text);
});

test("settings changes during capture affect only the next dictation", async () => {
  await workspace.settings.save({
    processingEnabled: false,
    dictionaryEnabled: true,
    dictionaryEntries: [{ written: "Fono", spoken: ["новый интерфейс"] }],
  });
  workspace.dictation.start();
  await workspace.settings.toggle("dictionaryEnabled", false);
  await workspace.dictation.finish();
  assert.ok(workspace.state.last.entry.text.includes("Fono"));
  workspace.dictation.start();
  await workspace.dictation.finish();
  assert.ok(workspace.state.last.entry.text.includes("новый интерфейс"));
});
