import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";
import { createSSRApp, h, nextTick } from "vue";
import { renderToString } from "vue/server-renderer";
import { WlConfig, WlToastService } from "@whitelife-core/ui-kit";
import { createRouter, createMemoryHistory } from "vue-router";
let rawDefaults;
let interactionKey, createInteraction;
let server,
  domain,
  usage,
  mapping,
  defaults,
  createWorkspace,
  workspaceKey,
  Page,
  storage;
before(async () => {
  storage = new Map();
  globalThis.localStorage = {
    getItem: (k) => storage.get(k) ?? null,
    setItem: (k, v) => storage.set(k, v),
  };
  globalThis.window = { addEventListener() {}, removeEventListener() {} };
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { hmr: false, middlewareMode: true },
  });
  domain = await server.ssrLoadModule(
    "/src/v3/features/history/domain/applicationContext.ts",
  );
  usage = await server.ssrLoadModule(
    "/src/v3/features/history/application/applicationUsage.ts",
  );
  mapping = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/mapping.ts",
  );
  ({ DEFAULT_SETTINGS: rawDefaults } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/ipcTypes.ts",
  ));
  ({ defaults } = await server.ssrLoadModule(
    "/src/v3/features/preferences/domain/preferences.ts",
  ));
  ({ createMockWorkspace: createWorkspace } = await server.ssrLoadModule(
    "/src/v3/app/mockWorkspace.ts",
  ));
  ({ workspaceKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/workspace.ts",
  ));
  ({ interactionKey, createInteraction } = await server.ssrLoadModule(
    "/src/v3/shared/application/interaction.ts",
  ));
  Page = (
    await server.ssrLoadModule(
      "/src/v3/features/history/presentation/HistoryPage.vue",
    )
  ).default;
});
after(async () => server?.close());
const context = (name = "Code", outcome = "sent") => ({
  before: { id: `exe:${name.toLowerCase()}.exe`, name },
  after: null,
  outcome,
  method: "clipboard",
  attempted: true,
});
const entry = (id, c) => ({
  id,
  text: "Text stays",
  title: "Title",
  createdAt: new Date().toISOString(),
  duration: 2,
  applicationContext: c,
});
test("legacy settings and entries have no invented application context", () => {
  assert.equal(defaults.applicationContextEnabled, false);
  assert.equal(
    mapping.historyFromNative({
      id: "old",
      text: "old text",
      device: null,
      created_at: new Date().toISOString(),
    }).applicationContext,
    undefined,
  );
  assert.equal(domain.applicationContextFromNative(null), undefined);
});
test("context decoder retains only allowed app fields and rejects paths or invalid identity", () => {
  const result = domain.applicationContextFromNative({
    ...context(),
    windowTitle: "PRIVATE",
    url: "PRIVATE",
    before: { ...context().before, path: "PRIVATE" },
  });
  assert.deepEqual(result, context());
  assert.ok(!JSON.stringify(result).includes("PRIVATE"));
  assert.equal(
    domain.applicationContextFromNative({
      ...context(),
      outcome: "confirmed_delivery",
    }),
    undefined,
  );
  assert.equal(
    domain.applicationContextFromNative({
      ...context(),
      before: { id: "exe:private.exe", name: "C:\\Private.exe" },
    }).before,
    null,
  );
});
test("native preference patch carries the independent opt-in flag", () => {
  const raw = mapping.applyPreferences(
    rawDefaults,
    { applicationContextEnabled: true },
    [],
  );
  assert.equal(raw.application_context_enabled, true);
  assert.equal(raw.analytics_enabled, rawDefaults.analytics_enabled);
  assert.equal(rawDefaults.application_context_enabled, false);
});
test("usage counts attempts, including blocked and unknown, without inferring delivery", () => {
  const entries = [
    entry("one", context()),
    entry("two", context("Code", "focus_changed")),
    entry("three", { ...context(), before: null }),
    entry("copy", {
      before: null,
      after: null,
      attempted: false,
      outcome: "not_attempted",
      method: null,
    }),
    entry("old"),
  ];
  const s = usage.applicationUsage(entries);
  assert.equal(s.total, 3);
  assert.equal(s.rows.find((r) => r.id === "exe:code.exe").attempts, 2);
  assert.equal(s.rows.find((r) => r.id === "exe:code.exe").share, 2 / 3);
  assert.equal(s.rows.find((r) => r.id === "unknown").attempts, 1);
  assert.ok(s.rows.every((r) => !("text" in r) && !("delivered" in r)));
});
test("filters use only the pre-send app, with explicit unknown and uncollected states", () => {
  const entries = [
    entry("code", {
      ...context(),
      after: { id: "exe:other.exe", name: "Other" },
      outcome: "focus_changed",
    }),
    entry("unknown", { ...context(), before: null }),
    entry("legacy"),
  ];
  assert.deepEqual(
    usage.filterByApplication(entries, "exe:code.exe").map((e) => e.id),
    ["code"],
  );
  assert.deepEqual(usage.filterByApplication(entries, "exe:other.exe"), []);
  assert.deepEqual(
    usage.filterByApplication(entries, "unknown").map((e) => e.id),
    ["unknown"],
  );
  assert.deepEqual(
    usage.filterByApplication(entries, "not_collected").map((e) => e.id),
    ["legacy"],
  );
  assert.equal(usage.applicationChoices(entries).length, 1);
  assert.deepEqual(usage.applicationUsage([]), { total: 0, rows: [] });
});
test("metadata erasure is targeted and preserves dictation text and LLM alternatives", async () => {
  const w = createWorkspace();
  try {
    const before = w.state.history.map((e) => e.text);
    const first = w.state.history[0];
    assert.ok(first.applicationContext);
    await w.history.clearApplicationContext(first.id);
    assert.equal(first.applicationContext, undefined);
    assert.ok(w.state.history.some((e) => e.applicationContext));
    await w.history.clearApplicationContext();
    assert.ok(w.state.history.every((e) => !e.applicationContext));
    assert.deepEqual(
      w.state.history.map((e) => e.text),
      before,
    );
  } finally {
    w.dispose();
  }
});
test("disabled context never renders app labels; enabled context exposes its observed state", async () => {
  const w = createWorkspace();
  try {
    w.state.history = [entry("one", context("SensitiveApp"))];
    const render = async () => {
      const app = createSSRApp({
        render: () => h(Page),
      });
      app.use(WlConfig).use(WlToastService);
      const router = createRouter({
        history: createMemoryHistory(),
        routes: [{ path: "/", component: { render: () => null } }],
      });
      await router.push("/");
      app.use(router);
      app.provide(workspaceKey, w);
      app.provide(interactionKey, createInteraction());
      return renderToString(app);
    };
    w.state.preferences.applicationContextEnabled = false;
    assert.ok(!(await render()).includes("SensitiveApp"));
    w.state.preferences.applicationContextEnabled = true;
    const html = await render();
    assert.ok(html.includes("SensitiveApp"));
    assert.ok(html.includes("Попытка вставки"));
    assert.ok(html.includes("Приём текста полем не подтверждён"));
  } finally {
    w.dispose();
  }
});
test("browser persistence stores the demo preference, never application metadata or transcripts", async () => {
  const w = createWorkspace();
  try {
    await w.settings.save({ applicationContextEnabled: true });
    await nextTick();
    const saved = JSON.stringify([...storage.values()]);
    assert.ok(!saved.includes("exe:code.exe"));
    assert.ok(!saved.includes("Text stays"));
    assert.ok(!saved.includes('"before"'));
  } finally {
    w.dispose();
    storage.clear();
  }
});
