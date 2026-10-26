import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";
import { createSSRApp, h } from "vue";
import { renderToString } from "vue/server-renderer";
import { createRouter, createMemoryHistory, RouterView } from "vue-router";
import { WlConfig, WlToastService } from "@whitelife-core/ui-kit";

let server,
  createWorkspace,
  Page,
  workspaceKey,
  interactionKey,
  catalog,
  sectionKeys,
  focusField,
  Overlay;
before(async () => {
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { hmr: false, middlewareMode: true },
  });
  ({ createMockWorkspace: createWorkspace } = await server.ssrLoadModule(
    "/src/v3/app/mockWorkspace.ts",
  ));
  ({ default: Page } = await server.ssrLoadModule(
    "/src/v3/features/preferences/presentation/SettingsPage.vue",
  ));
  ({ default: Overlay } = await server.ssrLoadModule(
    "/src/v3/features/preferences/presentation/OverlayFields.vue",
  ));
  ({ workspaceKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/workspace.ts",
  ));
  ({ interactionKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/interaction.ts",
  ));
  catalog = await server.ssrLoadModule(
    "/src/v3/features/preferences/application/catalog.ts",
  );
  ({ sectionKeys } = await server.ssrLoadModule(
    "/src/v3/features/preferences/domain/preferences.ts",
  ));
  ({ focusField } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/focus.ts",
  ));
});
after(async () => await server?.close());

async function renderSection(
  section,
  component = Page,
  props = {},
  { liveAvailable = false, sampleToolsAvailable = false } = {},
) {
  const previousWindow = globalThis.window;
  globalThis.window = { addEventListener() {}, removeEventListener() {} };
  const workspace = createWorkspace();
  workspace.native = true;
  workspace.wakeSamples.state.live.available = liveAvailable;
  workspace.wakeSamples.state.live.sampleToolsAvailable = sampleToolsAvailable;
  const initial = JSON.stringify(workspace.state.preferences);
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: "/settings/:section", component, props: () => props }],
  });
  const app = createSSRApp({ render: () => h(RouterView) });
  app.use(WlConfig);
  app.use(WlToastService);
  app.use(router);
  app.provide(workspaceKey, workspace);
  app.provide(interactionKey, {
    confirm: async () => false,
    state: { quick: null, confirmation: null },
  });
  try {
    await router.push("/settings/" + section);
    const html = await renderToString(app);
    assert.equal(
      JSON.stringify(workspace.state.preferences),
      initial,
      "Opening a section cannot change preferences or consent",
    );
    return html;
  } finally {
    workspace.dispose();
    if (previousWindow === undefined) delete globalThis.window;
    else globalThis.window = previousWindow;
  }
}

test("all seven sections start with Advanced closed and preserve saved preferences", async () => {
  for (const { id } of catalog.sections) {
    const html = await renderSection(id);
    const tag = html.match(
      new RegExp(`<details\\b[^>]*id="${id}-advanced"[^>]*>`),
    );
    assert.ok(tag, `${id} must retain an Advanced section`);
    assert.doesNotMatch(tag[0], /\sopen(?:[\s=>])/);
  }
});

test("settings search still addresses every rendered setting, including nested and disabled features", async () => {
  const html = {};
  for (const { id } of catalog.sections) html[id] = await renderSection(id);
  const missing = [];
  for (const item of catalog.searchIndex) {
    if (["wake-live", "wake-samples"].includes(item.field)) continue;
    const ids = [...html[item.section].matchAll(/\bid="([^"]+)"/g)].map(
      (match) => match[1],
    );
    const fallback = catalog.settingFocusFallbacks[item.field];
    if (!(ids.includes(item.field) || (fallback && ids.includes(fallback))))
      missing.push(`${item.label}: missing ${item.section}/${item.field}`);
  }
  assert.deepEqual(missing, []);
});

test("moved controls remain saveable in their new section and old links retain a destination", () => {
  for (const key of ["recordingSoundsVolume", "recordingSoundsEnabled"]) {
    assert.ok(sectionKeys.activation.includes(key));
    assert.equal(sectionKeys.audio.includes(key), false);
    assert.equal(catalog.relocatedSettings[key], "activation");
  }
  for (const key of [
    "trainerModel",
    "trainerProfile",
    "trainerScope",
    "trainerAiEnabled",
    "cloudConsent",
  ]) {
    assert.ok(sectionKeys.privacy.includes(key));
    assert.equal(sectionKeys.processing.includes(key), false);
    assert.equal(catalog.relocatedSettings[key], "privacy");
  }
  assert.equal(catalog.relocatedSettings["wake-samples"], "diagnostics");
  assert.equal(
    sectionKeys.overlay.some(
      (key) => key.startsWith("processing") || key === "language",
    ),
    false,
  );
});

test("live wake stays in activation and its recording tools move to diagnostics behind capability checks", async () => {
  const activation = await renderSection(
    "activation",
    Page,
    {},
    { liveAvailable: true },
  );
  assert.match(activation, /id="wake-live"/);
  assert.doesNotMatch(activation, /id="wake-samples"/);
  const diagnostics = await renderSection(
    "diagnostics",
    Page,
    {},
    { liveAvailable: true, sampleToolsAvailable: true },
  );
  assert.match(diagnostics, /id="wake-tools"/);
  assert.match(diagnostics, /id="wake-samples"/);
  const releaseDiagnostics = await renderSection(
    "diagnostics",
    Page,
    {},
    { liveAvailable: true },
  );
  assert.doesNotMatch(releaseDiagnostics, /id="wake-tools"|id="wake-samples"/);
  assert.doesNotMatch(await renderSection("diagnostics"), /id="wake-samples"/);
});

test("search opens every containing disclosure and can reveal a disabled feature without enabling it", () => {
  const previous = {
    document: globalThis.document,
    details: globalThis.HTMLDetailsElement,
    timer: globalThis.setTimeout,
  };
  const opened = [];
  class Details {
    constructor(name, parentElement = null) {
      this.name = name;
      this.parentElement = parentElement;
      this._open = false;
    }
    set open(value) {
      this._open = value;
      opened.push(this.name);
    }
    get open() {
      return this._open;
    }
  }
  const outer = new Details("advanced"),
    inner = new Details("dictionary", outer);
  let focused = 0,
    scrolled = 0;
  const target = {
    parentElement: inner,
    matches: () => true,
    scrollIntoView() {
      scrolled++;
    },
    focus() {
      focused++;
    },
    classList: { add() {}, remove() {} },
  };
  globalThis.HTMLDetailsElement = Details;
  globalThis.document = {
    getElementById: (id) => (id === "feature-toggle" ? target : null),
  };
  globalThis.setTimeout = () => 0;
  try {
    focusField("hidden-control", "feature-toggle");
    assert.deepEqual(opened, ["advanced", "dictionary"]);
    assert.equal(focused, 1);
    assert.equal(scrolled, 1);
  } finally {
    if (previous.document === undefined) delete globalThis.document;
    else globalThis.document = previous.document;
    if (previous.details === undefined) delete globalThis.HTMLDetailsElement;
    else globalThis.HTMLDetailsElement = previous.details;
    globalThis.setTimeout = previous.timer;
  }
});

test("interactive overlay examples cannot change actual processing or recognition preferences", async () => {
  let bindings;
  const Probe = {
    ...Overlay,
    setup(props, context) {
      bindings = Overlay.setup(props, context);
      return bindings;
    },
  };
  const workspace = createWorkspace();
  const preferences = structuredClone(
    JSON.parse(JSON.stringify(workspace.state.preferences)),
  );
  workspace.dispose();
  const before = JSON.stringify(preferences);
  await renderSection("overlay", Probe, { modelValue: preferences });
  bindings.changePreviewProcessing({
    preset: "formal",
    targetLanguage: "en",
    processingEnabled: false,
    translationEnabled: true,
    recognitionLanguage: "en",
  });
  assert.equal(JSON.stringify(preferences), before);
  assert.equal(bindings.previewPreferences.value.processingMode, "formal");
  assert.equal(bindings.previewPreferences.value.language, "en");
});
