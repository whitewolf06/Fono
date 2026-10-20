import assert from "node:assert/strict";
import { before, beforeEach, afterEach, after, test } from "node:test";
import { createServer } from "vite";
let server,
  domain,
  mapping,
  defaults,
  createWorkspace,
  workspace,
  storage,
  createNativeSnippetTransfer,
  readPreferences;
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
  domain = await server.ssrLoadModule(
    "/src/v3/features/snippets/domain/snippets.ts",
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
  ({ createNativeSnippetTransfer } = await server.ssrLoadModule(
    "/src/v3/features/snippets/infrastructure/nativeSnippets.ts",
  ));
  ({ readPreferences } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/browser.ts",
  ));
});
beforeEach(() => {
  storage.clear();
});
afterEach(() => {
  workspace?.dispose();
  workspace = undefined;
});
after(async () => {
  await server?.close();
  delete globalThis.localStorage;
});
const entry = () => ({
  trigger: "моя подпись",
  text: "Дмитрий\n{date} {clipboard}",
  enabled: true,
});

test("normalization keeps whole-utterance matching and question punctuation distinct", () => {
  assert.equal(domain.normalizeTrigger("  МОЯ    ПОДПИСЬ.! "), "моя подпись");
  assert.notEqual(
    domain.normalizeTrigger("моя подпись?"),
    domain.normalizeTrigger(entry().trigger),
  );
  for (const text of [
    "какая моя подпись",
    "моя подпись и дата",
    "это моя подпись",
  ])
    assert.notEqual(
      domain.normalizeTrigger(text),
      domain.normalizeTrigger(entry().trigger),
    );
  assert.ok(
    domain.validateSnippets([
      { ...entry(), trigger: "Ёж!" },
      { ...entry(), trigger: "еж." },
    ]),
  );
});

test("JSON imports preserve literal data, default enabled and reject unknown fields, versions and limits", () => {
  const literal = {
    trigger: "текст",
    text: "<script>throw 1</script> ${run()} {date}",
    enabled: false,
  };
  assert.deepEqual(domain.parseSnippetJson(domain.snippetJson([literal])), [
    literal,
  ]);
  assert.equal(
    domain.parseSnippetJson(
      '{"version":1,"snippets":[{"trigger":"текст","text":"ok"}]}',
    )[0].enabled,
    true,
  );
  for (const json of [
    '{"version":2,"snippets":[]}',
    '{"version":1,"snippets":[],"run":"cmd"}',
    '{"version":1,"snippets":[{"trigger":"x","text":"a","command":"cmd"}]}',
  ])
    assert.throws(() => domain.parseSnippetJson(json));
  assert.throws(() =>
    domain.parseSnippetJson(" ".repeat(domain.snippetLimits.jsonBytes + 1)),
  );
  assert.ok(domain.validateSnippets([{ ...entry(), text: "a".repeat(8193) }]));
  assert.ok(domain.validateSnippets([{ ...entry(), trigger: "вопрос?" }]));
});

test("native settings roundtrip clones templates and keeps disabled entries; old settings default off", () => {
  const old = { ...defaults };
  delete old.snippets;
  delete old.snippets_enabled;
  assert.equal(mapping.preferencesFromNative(old).snippetsEnabled, false);
  assert.deepEqual(mapping.preferencesFromNative(old).snippets, []);
  const native = mapping.applyPreferences(
    defaults,
    { snippetsEnabled: true, snippets: [entry()] },
    [],
  );
  const preferences = mapping.preferencesFromNative(native);
  assert.equal(preferences.snippetsEnabled, true);
  assert.deepEqual(preferences.snippets, [entry()]);
  preferences.snippets[0].text = "changed";
  assert.equal(native.snippets[0].text, entry().text);
  assert.equal(
    mapping.applyPreferences(native, { snippetsEnabled: false }, []).snippets
      .length,
    1,
  );
});

test("mock CRUD stays in memory across settings saves and invalid imports cannot replace it", async () => {
  workspace = createWorkspace();
  await workspace.settings.save({ snippets: [entry()] });
  await workspace.settings.toggle("snippetsEnabled", true);
  assert.deepEqual(workspace.state.preferences.snippets, [entry()]);
  assert.ok(![...storage.values()].join().includes("моя подпись"));
  assert.ok(![...storage.values()].join().includes("{clipboard}"));
  await workspace.settings.toggle("snippetsEnabled", false);
  assert.equal(workspace.state.preferences.snippets.length, 1);
  assert.throws(() =>
    domain.parseSnippetJson('{"version":1,"snippets":[{"trigger":"bad"}]}'),
  );
  assert.equal(workspace.state.preferences.snippets.length, 1);
  assert.deepEqual(readPreferences().snippets, []);
  workspace.dispose();
  workspace = createWorkspace();
  assert.deepEqual(workspace.state.preferences.snippets, []);
  assert.equal(workspace.state.preferences.snippetsEnabled, false);
});

test("transfer port sends literal templates for explicit export and import returns only a draft", async () => {
  const calls = [];
  const transfer = createNativeSnippetTransfer(async (command, args) => {
    calls.push({ command, args });
    return command === "import_snippets" ? [entry()] : true;
  });
  assert.deepEqual(await transfer.import(), [entry()]);
  assert.equal(await transfer.export([entry()]), true);
  assert.deepEqual(calls, [
    { command: "import_snippets", args: undefined },
    { command: "export_snippets", args: { entries: [entry()] } },
  ]);
});
