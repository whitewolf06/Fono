import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";
let server, presets, createLookup, nativeSettings, createWorkspace, defaults;
const calls = [],
  storage = new Map();
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
        return ["actual-model-id"];
      },
    },
  };
  server = await createServer({
    appType: "custom",
    server: { middlewareMode: true, hmr: false },
  });
  const load = (p) => server.ssrLoadModule("/src/v3/" + p);
  presets = await load("features/preferences/domain/localLlmPresets.ts");
  ({ createLocalModelsLookup: createLookup } = await load(
    "features/preferences/application/localModels.ts",
  ));
  ({ nativeSettings } = await load("shared/infrastructure/native/settings.ts"));
  ({ createMockWorkspace: createWorkspace } = await load(
    "app/mockWorkspace.ts",
  ));
  ({ defaults } = await load("features/preferences/domain/preferences.ts"));
});
after(async () => {
  await server?.close();
  delete globalThis.window;
  delete globalThis.localStorage;
});
const draft = () => ({
  id: "",
  name: "Draft",
  provider: "custom",
  location: "cloud",
  url: "https://example.invalid/v1",
  model: "old-model",
  apiKey: "PRIVATE KEY",
  hasApiKey: true,
});
function deferred() {
  let resolve, reject;
  const promise = new Promise((ok, no) => {
    resolve = ok;
    reject = no;
  });
  return { promise, resolve, reject };
}

test("templates use loopback defaults, clear unrelated draft credentials and never retarget a saved ID", () => {
  for (const preset of ["ollama", "lmstudio"]) {
    const original = draft(),
      next = presets.applyLocalPreset(original, preset);
    assert.equal(presets.localPresetFor(next), preset);
    assert.equal(
      next.url,
      preset === "ollama"
        ? "http://127.0.0.1:11434/v1"
        : "http://127.0.0.1:1234/v1",
    );
    assert.equal(next.apiKey, "");
    assert.equal(next.hasApiKey, false);
    assert.equal(next.model, "");
    assert.equal(original.apiKey, "PRIVATE KEY");
    assert.equal(
      presets.localPresetFor({ ...next, url: "https://example.invalid/v1" }),
      null,
    );
    assert.equal(presets.localPresetFor({ ...next, location: "cloud" }), null);
    assert.throws(() =>
      presets.applyLocalPreset({ ...original, id: "saved" }, preset),
    );
  }
});
test("lookup is idle until explicit check, normalizes real IDs and distinguishes an empty available server", async () => {
  let reads = 0,
    result = [" small ", "small", "", "large"];
  const lookup = createLookup({
    listLocalModels: async () => {
      reads++;
      return result;
    },
  });
  assert.equal(reads, 0);
  assert.equal(lookup.state.status, "idle");
  await lookup.check("ollama");
  assert.equal(reads, 1);
  assert.equal(lookup.state.status, "available");
  assert.deepEqual([...lookup.state.models], ["small", "large"]);
  result = [];
  await lookup.check("lmstudio");
  assert.equal(lookup.state.status, "empty");
});
test("changing template or closing invalidates late success and failure without overriding a newer result", async () => {
  const first = deferred(),
    second = deferred();
  let count = 0;
  const lookup = createLookup({
    listLocalModels: () => (++count === 1 ? first.promise : second.promise),
  });
  const old = lookup.check("ollama");
  lookup.reset();
  const next = lookup.check("lmstudio");
  second.resolve(["new-model"]);
  await next;
  first.reject(new Error("PRIVATE SERVER DETAILS"));
  await old;
  assert.equal(lookup.state.status, "available");
  assert.deepEqual([...lookup.state.models], ["new-model"]);
  const closing = deferred(),
    closed = createLookup({ listLocalModels: () => closing.promise });
  const late = closed.check("ollama");
  closed.reset();
  closing.resolve(["stale"]);
  await late;
  assert.equal(closed.state.status, "idle");
  assert.deepEqual([...closed.state.models], []);
});
test("native draft discovery reuses models IPC and sends no key, draft text, save or generation command", async () => {
  calls.length = 0;
  const port = nativeSettings({ state: { preferences: { ...defaults } } });
  assert.deepEqual(await port.listLocalModels("ollama"), ["actual-model-id"]);
  await port.listModels("existing-profile");
  assert.deepEqual(calls, [
    { command: "list_llm_profile_models", args: { localPreset: "ollama" } },
    {
      command: "list_llm_profile_models",
      args: { profileId: "existing-profile" },
    },
  ]);
});
test("a missing server leaves profiles and dictation preferences intact; retry succeeds in the demo port", async () => {
  const workspace = createWorkspace();
  try {
    const before = JSON.stringify({
      profiles: workspace.state.profiles,
      preferences: workspace.state.preferences,
    });
    workspace.state.aiAvailable = false;
    const lookup = createLookup(workspace.settings);
    await lookup.check("ollama");
    assert.equal(lookup.state.status, "unavailable");
    assert.equal(
      JSON.stringify({
        profiles: workspace.state.profiles,
        preferences: workspace.state.preferences,
      }),
      before,
    );
    workspace.state.aiAvailable = true;
    await lookup.check("lmstudio");
    assert.equal(lookup.state.status, "available");
  } finally {
    workspace.dispose();
    storage.clear();
  }
});
