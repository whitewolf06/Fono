import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";
import { createSSRApp, h, reactive } from "vue";
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
  Overlay,
  SourceFields,
  AudioFields,
  RemoteModels,
  createSourceController,
  connectionErrorText;
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
  ({ default: SourceFields } = await server.ssrLoadModule(
    "/src/v3/features/preferences/presentation/RecognitionSourceFields.vue",
  ));
  ({ default: AudioFields } = await server.ssrLoadModule(
    "/src/v3/features/preferences/presentation/AudioFields.vue",
  ));
  ({ default: RemoteModels } = await server.ssrLoadModule(
    "/src/v3/features/preferences/presentation/ModelManager.vue",
  ));
  ({
    createRecognitionSourceController: createSourceController,
    connectionErrorText,
  } = await server.ssrLoadModule(
    "/src/v3/features/preferences/application/recognitionSource.ts",
  ));
});
after(async () => await server?.close());

function sshSettings() {
  return {
    host: "server.example",
    port: 22,
    user: "fono",
    identityFile: "C:/keys/private",
    serverPort: 17832,
    hostKey: "",
  };
}
const sshCandidate = {
  kind: "ok",
  hostKey: "ssh-ed25519 " + "A".repeat(68),
  fingerprint: "SHA256:" + "A".repeat(43),
};

test("SSH host probe is a draft observation and only explicit trust pins the current address", async () => {
  const settings = sourceSettings();
  settings.ssh = sshSettings();
  const controller = createSourceController({
    settings: () => settings,
    port: sourcePort({ probeSshHost: async () => sshCandidate }),
  });
  await controller.probeSshHost();
  assert.equal(settings.ssh.hostKey, "");
  assert.equal(
    controller.state.hostKeyCandidate.fingerprint,
    sshCandidate.fingerprint,
  );
  controller.trustHostKey();
  assert.equal(settings.ssh.hostKey, sshCandidate.hostKey);
  assert.equal(controller.state.hostKeyCandidate, null);
  settings.ssh.host = "replacement.example";
  controller.invalidate();
  assert.equal(
    settings.ssh.hostKey,
    "",
    "a different SSH host cannot inherit an accepted pin",
  );
  controller.trustHostKey();
  assert.equal(settings.ssh.hostKey, "");
  controller.dispose();
});

test("host-key retrieval failures explain the discovery step without blaming account credentials", async () => {
  for (const probeSshHost of [
    async () => ({ kind: "error", error: "ssh_connection_failed" }),
    async () => {
      throw new Error("PRIVATE_SSH_SERVER_STDERR");
    },
  ]) {
    const settings = sourceSettings();
    settings.ssh = sshSettings();
    const controller = createSourceController({
      settings: () => settings,
      port: sourcePort({ probeSshHost }),
    });
    await controller.probeSshHost();
    assert.match(controller.state.sshError, /Не удалось получить ключ сервера/);
    assert.match(
      controller.state.sshError,
      /Личный ключ и токен на этом шаге не используются/,
    );
    assert.doesNotMatch(controller.state.sshError, /PRIVATE_|пользовател/);
    assert.equal(settings.ssh.hostKey, "");
    assert.equal(controller.state.hostKeyCandidate, null);
    assert.equal(controller.state.probingHost, false);
    controller.dispose();
  }
});

test("SSH host probes and identity dialogs cannot update a changed or disposed draft", async () => {
  for (const change of ["host", "port", "dispose"]) {
    const settings = sourceSettings();
    settings.ssh = sshSettings();
    const pending = deferred();
    const controller = createSourceController({
      settings: () => settings,
      port: sourcePort({ probeSshHost: () => pending.promise }),
    });
    const checking = controller.probeSshHost();
    if (change === "dispose") controller.dispose();
    else {
      settings.ssh[change] = change === "host" ? "changed.example" : 2222;
      controller.invalidate();
    }
    pending.resolve(sshCandidate);
    await checking;
    assert.equal(controller.state.hostKeyCandidate, null);
    assert.equal(settings.ssh.hostKey, "");
    controller.dispose();
  }
  const settings = sourceSettings();
  settings.ssh = sshSettings();
  const pending = deferred();
  const controller = createSourceController({
    settings: () => settings,
    port: sourcePort({ pickIdentityFile: () => pending.promise }),
  });
  const choosing = controller.pickIdentityFile();
  settings.ssh.user = "replacement";
  controller.invalidate();
  pending.resolve("C:/keys/late-private-key");
  await choosing;
  assert.equal(settings.ssh.identityFile, "C:/keys/private");
  controller.dispose();
});

test("every SSH setting participates in the pending connection check signature", async () => {
  for (const key of [
    "host",
    "port",
    "user",
    "identityFile",
    "serverPort",
    "hostKey",
  ]) {
    const settings = sourceSettings();
    settings.ssh = { ...sshSettings(), hostKey: sshCandidate.hostKey };
    const pending = deferred();
    let request;
    const controller = createSourceController({
      settings: () => settings,
      port: sourcePort({
        test: async (input) => {
          request = input;
          return pending.promise;
        },
      }),
    });
    const checking = controller.check();
    settings.ssh[key] = typeof settings.ssh[key] === "number" ? 42 : "changed";
    pending.resolve({ kind: "ok", server: remoteServer() });
    await checking;
    assert.equal(controller.state.result, null, key);
    assert.notEqual(
      request.ssh[key],
      settings.ssh[key],
      "the native request owns an immutable SSH snapshot",
    );
    controller.dispose();
  }
});

test("SSH settings render key choice and explicit host trust without a manual tunnel URL", async () => {
  const settings = sourceSettings();
  settings.ssh = sshSettings();
  const html = await renderRecognition(SourceFields, {
    settings,
    port: sourcePort({ probeSshHost: async () => sshCandidate }),
    prepare: async (bindings) => {
      await bindings.controller.probeSshHost();
    },
  });
  assert.match(html, /Адрес SSH-сервера/);
  assert.match(html, /Выбрать файл ключа/);
  assert.match(html, /Доверять этому ключу/);
  assert.match(html, /SHA256:/);
  assert.match(html, /id="remote-server-name"/);
  assert.doesNotMatch(html, /id="remote-server-url"/);
  assert.doesNotMatch(html, /ssh -|powershell|PRIVATE_TOKEN/);
});

test("replacing the preferences draft preserves a trusted SSH address while editing its host clears the pin", async () => {
  const settings = sourceSettings();
  settings.ssh = { ...sshSettings(), hostKey: sshCandidate.hostKey };
  await renderRecognition(SourceFields, {
    settings,
    reactiveModel: true,
    prepare: (bindings) => {
      const current = bindings.connection.value;
      bindings.draft.value.recognitionSource = {
        ...current,
        ssh: { ...current.ssh },
      };
      assert.equal(
        bindings.connection.value.ssh.hostKey,
        sshCandidate.hostKey,
        "Save/Reset replaces a draft without revoking the same host pin",
      );
      bindings.connection.value.ssh.host = "changed.example";
      assert.equal(bindings.connection.value.ssh.hostKey, "");
    },
  });
});

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

const remoteServer = () => ({
  version: "0.8.35",
  apiVersion: 2,
  state: "ready",
  devices: [
    { id: "cuda:0", name: "Demo GPU", backend: "cuda", available: true },
  ],
  models: [
    {
      id: "large_turbo",
      label: "Remote Whisper",
      installed: true,
      state: "ready",
      languages: ["ru", "en"],
      device: "cuda:0",
    },
  ],
});
const sourceSettings = () => ({
  source: "fono_server",
  url: "https://server.example",
  displayName: "Kari",
  model: "large_turbo",
  caPem: "",
  fallback: "none",
  fallbackModel: "",
});
function sourcePort(overrides = {}) {
  return {
    status: async () => ({ hasToken: false }),
    test: async () => ({ kind: "ok", server: remoteServer() }),
    saveToken: async () => {},
    clearToken: async () => {},
    ...overrides,
  };
}
function deferred() {
  let resolve;
  return {
    promise: new Promise((done) => (resolve = done)),
    resolve: (...args) => resolve(...args),
  };
}

test("recognition checks show every typed failure and do not persist settings or a draft credential", async () => {
  for (const error of [
    "invalid_endpoint",
    "not_configured",
    "network",
    "timeout",
    "unauthorized",
    "incompatible_version",
    "unsupported_capabilities",
    "model_not_found",
    "invalid_response",
    "busy",
    "server_failed",
  ]) {
    const settings = sourceSettings();
    const before = JSON.stringify(settings);
    let writes = 0;
    const controller = createSourceController({
      settings: () => settings,
      port: sourcePort({
        test: async () => ({ kind: "error", error }),
        saveToken: async () => writes++,
      }),
    });
    controller.setToken("DRAFT_ONLY");
    await controller.check();
    assert.equal(controller.state.result.error, error);
    assert.ok(connectionErrorText(error).length > 12);
    assert.equal(controller.state.checking, false);
    assert.equal(JSON.stringify(settings), before);
    assert.equal(writes, 0);
    assert.doesNotMatch(JSON.stringify(controller.state.result), /DRAFT_ONLY/);
    controller.dispose();
    assert.equal(controller.state.token, "");
  }
});

test("native compatibility mapping distinguishes legacy API and unsupported capabilities before exposing metadata", async () => {
  const { remoteConnectionFromNative } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/recognitionSource.ts",
  );
  for (const api_version of [0, 1]) {
    assert.deepEqual(
      remoteConnectionFromNative({
        kind: "ok",
        server: {
          api_version,
          token: "PRIVATE_TOKEN",
          address: "https://PRIVATE.invalid",
        },
      }),
      { kind: "error", error: "incompatible_version" },
    );
  }
  assert.deepEqual(
    remoteConnectionFromNative({
      kind: "error",
      error: "unsupported_capabilities",
      details: "PRIVATE_TOKEN https://PRIVATE.invalid",
    }),
    { kind: "error", error: "unsupported_capabilities" },
  );
  const compatible = {
    version: "0.8.37",
    api_version: 2,
    state: "ready",
    devices: [],
    models: [],
  };
  assert.equal(
    remoteConnectionFromNative({ kind: "ok", server: compatible }).kind,
    "ok",
  );
  for (const api_version of ["1", null, 2.5]) {
    assert.deepEqual(
      remoteConnectionFromNative({
        kind: "ok",
        server: { ...compatible, api_version },
      }),
      { kind: "error", error: "invalid_response" },
    );
  }
});

test("compatibility alerts distinguish updating the server from unsupported recognition features", async () => {
  const { bindMessageLocale, uiLocale } = await server.ssrLoadModule(
    "/src/v3/shared/domain/locale.ts",
  );
  const previous = uiLocale();
  try {
    bindMessageLocale(() => "ru");
    for (const [error, text] of [
      ["incompatible_version", /Эта версия Fono Server несовместима/],
      [
        "unsupported_capabilities",
        /Сервер не поддерживает нужные возможности распознавания/,
      ],
    ]) {
      const html = await renderRecognition(SourceFields, {
        port: sourcePort({ test: async () => ({ kind: "error", error }) }),
        prepare: (bindings) => bindings.controller.check(),
      });
      assert.match(html, /role="alert"/);
      assert.match(html, text);
      assert.match(html, /Обновите/);
      assert.doesNotMatch(html, /PRIVATE|Authorization:|Bearer /);
    }
    bindMessageLocale(() => "en");
    assert.match(
      connectionErrorText("incompatible_version"),
      /Update the server/,
    );
    assert.match(
      connectionErrorText("unsupported_capabilities"),
      /Update Fono Server/,
    );
  } finally {
    bindMessageLocale(() => previous);
  }
});

test("changed address, model, token and disposed form cannot accept a late connection reply", async () => {
  for (const change of ["url", "model", "token", "dispose"]) {
    const settings = sourceSettings();
    const pending = deferred();
    const published = [];
    const controller = createSourceController({
      settings: () => settings,
      port: sourcePort({ test: () => pending.promise }),
      checked: (value) => published.push(value),
    });
    const checking = controller.check();
    if (change === "token") controller.setToken("NEW_DRAFT");
    else if (change === "dispose") controller.dispose();
    else {
      settings[change] = "changed";
      controller.invalidate(change !== "model");
    }
    pending.resolve({ kind: "ok", server: remoteServer() });
    await checking;
    assert.equal(controller.state.server, null);
    assert.equal(controller.state.result, null);
    assert.ok(published.every((value) => value === null));
    controller.dispose();
  }
});

test("a fake successful reply missing the selected model retains inventory but cannot validate the connection", async () => {
  const settings = sourceSettings();
  settings.model = "removed_model";
  let request;
  const checked = [];
  const controller = createSourceController({
    settings: () => settings,
    checked: (value) => checked.push(value),
    port: sourcePort({
      test: async (input) => {
        request = input;
        return { kind: "ok", server: remoteServer() };
      },
    }),
  });
  await controller.check();
  assert.equal(
    request.model,
    "removed_model",
    "the selected model must reach the authoritative compatibility check",
  );
  assert.equal(controller.state.result.error, "model_not_found");
  assert.equal(controller.state.server.models[0].id, "large_turbo");
  assert.deepEqual(checked, [null]);
  settings.model = "large_turbo";
  controller.invalidate(false);
  assert.ok(controller.state.server);
  await controller.check();
  assert.equal(controller.state.result.kind, "ok");
  controller.dispose();
});

test("a compatible selected model is checked in one metadata request", async () => {
  const settings = sourceSettings();
  const requests = [];
  const checked = [];
  const controller = createSourceController({
    settings: () => settings,
    checked: (value) => checked.push(value),
    port: sourcePort({
      test: async (input) => {
        requests.push(input);
        return { kind: "ok", server: remoteServer() };
      },
    }),
  });
  controller.setToken("DRAFT_CREDENTIAL");
  checked.length = 0;
  await controller.check();
  assert.equal(requests.length, 1);
  assert.deepEqual(requests[0], {
    url: settings.url,
    token: "DRAFT_CREDENTIAL",
    model: settings.model,
  });
  assert.equal(controller.state.result.kind, "ok");
  assert.equal(checked.length, 1);
  assert.equal(checked[0].models[0].id, settings.model);
  assert.doesNotMatch(
    JSON.stringify(controller.state.result),
    /DRAFT_CREDENTIAL/,
  );
  controller.dispose();
});

test("a rejected selected model retains its error while separate metadata inventory offers replacements", async () => {
  for (const error of ["unsupported_capabilities", "model_not_found"]) {
    const settings = sourceSettings();
    settings.model = "unsupported_model";
    const requests = [];
    const checked = [];
    const controller = createSourceController({
      settings: () => settings,
      checked: (value) => checked.push(value),
      port: sourcePort({
        test: async (input) => {
          requests.push(input);
          return input.model === "unsupported_model"
            ? { kind: "error", error }
            : { kind: "ok", server: remoteServer() };
        },
      }),
    });
    const original = JSON.stringify(settings);
    await controller.check();
    assert.equal(requests.length, 2);
    assert.equal(requests[0].model, "unsupported_model");
    assert.equal(requests[1].model, undefined);
    assert.deepEqual(controller.state.result, { kind: "error", error });
    assert.equal(controller.state.server.models[0].id, "large_turbo");
    assert.deepEqual(
      checked,
      [null],
      "inventory is not selected-model compatibility confirmation",
    );
    assert.equal(JSON.stringify(settings), original);
    settings.model = "large_turbo";
    controller.invalidate(false);
    assert.equal(controller.state.server.models[0].id, "large_turbo");
    await controller.check();
    assert.equal(requests.length, 3);
    assert.equal(controller.state.result.kind, "ok");
    controller.dispose();
  }
});

test("late replacement inventory cannot overwrite a changed connection or a newer check", async () => {
  for (const change of ["url", "model", "token", "dispose", "new_check"]) {
    const settings = sourceSettings();
    const pending = deferred();
    const enteredInventory = deferred();
    const checked = [];
    const controller = createSourceController({
      settings: () => settings,
      checked: (value) => checked.push(value),
      port: sourcePort({
        test: async (input) => {
          if (!input.model) {
            enteredInventory.resolve();
            return pending.promise;
          }
          return input.model === "replacement"
            ? {
                kind: "ok",
                server: {
                  ...remoteServer(),
                  models: [{ ...remoteServer().models[0], id: "replacement" }],
                },
              }
            : { kind: "error", error: "unsupported_capabilities" };
        },
      }),
    });
    const first = controller.check();
    await enteredInventory.promise;
    if (change === "token") controller.setToken("CHANGED_CREDENTIAL");
    else if (change === "dispose") controller.dispose();
    else {
      settings[change === "new_check" ? "model" : change] =
        change === "new_check" ? "replacement" : "changed";
      controller.invalidate();
    }
    if (change === "new_check") await controller.check();
    pending.resolve({ kind: "ok", server: remoteServer() });
    await first;
    if (change === "new_check") {
      assert.equal(controller.state.result.kind, "ok");
      assert.equal(controller.state.server.models[0].id, "replacement");
      assert.equal(checked.at(-1).models[0].id, "replacement");
    } else {
      if (change === "dispose")
        assert.deepEqual(controller.state.result, {
          kind: "error",
          error: "unsupported_capabilities",
        });
      else assert.equal(controller.state.result, null);
      assert.equal(controller.state.server, null);
      assert.ok(checked.every((value) => value === null));
    }
    controller.dispose();
  }
});

test("inventory network errors never replace the selected model compatibility failure", async () => {
  const controller = createSourceController({
    settings: sourceSettings,
    port: sourcePort({
      test: async (input) => {
        if (!input.model) throw Error("PRIVATE_NETWORK_ERROR");
        return { kind: "error", error: "unsupported_capabilities" };
      },
    }),
  });
  await controller.check();
  assert.deepEqual(controller.state.result, {
    kind: "error",
    error: "unsupported_capabilities",
  });
  assert.equal(controller.state.server, null);
  assert.doesNotMatch(JSON.stringify(controller.state.result), /PRIVATE/);
  controller.dispose();
});

test("credential save is explicit, clears the form and prevents late status from undoing it", async () => {
  const pending = deferred();
  const saved = [];
  const settings = sourceSettings();
  const before = JSON.stringify(settings);
  const controller = createSourceController({
    settings: () => settings,
    port: sourcePort({
      status: () => pending.promise,
      saveToken: async (value) => saved.push(value),
    }),
  });
  const loading = controller.loadStatus();
  controller.setToken("TRANSIENT_SECRET");
  await controller.check();
  assert.equal(saved.length, 0);
  await controller.saveToken();
  pending.resolve({ hasToken: false });
  await loading;
  assert.deepEqual(saved, ["TRANSIENT_SECRET"]);
  assert.equal(controller.state.hasToken, true);
  assert.equal(controller.state.token, "");
  assert.equal(
    controller.state.result.kind,
    "ok",
    "save keeps the just-tested observation",
  );
  assert.doesNotMatch(JSON.stringify(controller.state), /TRANSIENT_SECRET/);
  assert.equal(JSON.stringify(settings), before);
  await controller.clearToken();
  assert.equal(controller.state.hasToken, false);
  assert.equal(controller.state.server, null);
  assert.equal(controller.state.result, null);
  controller.dispose();
});

async function renderRecognition(
  Component,
  {
    settings = sourceSettings(),
    port = sourcePort(),
    prepare,
    props = {},
    native = true,
    reactiveModel = false,
  } = {},
) {
  const previousWindow = globalThis.window;
  globalThis.window = { addEventListener() {}, removeEventListener() {} };
  const workspace = createWorkspace();
  workspace.native = native;
  workspace.recognition = port;
  workspace.state.preferences.recognitionSource = settings;
  const initialModel = JSON.parse(JSON.stringify(workspace.state.preferences));
  const model = reactiveModel ? reactive(initialModel) : initialModel;
  const Probe = prepare
    ? {
        ...Component,
        async setup(componentProps, context) {
          const bindings = Component.setup(componentProps, context);
          await prepare(bindings);
          return bindings;
        },
      }
    : Component;
  const app = createSSRApp({
    render: () => h(Probe, { modelValue: model, ...props }),
  });
  app.use(WlConfig);
  app.use(WlToastService);
  app.provide(workspaceKey, workspace);
  app.provide(interactionKey, {
    confirm: async () => false,
    state: { quick: null, confirmation: null },
  });
  try {
    return await renderToString(app);
  } finally {
    workspace.dispose();
    if (previousWindow === undefined) delete globalThis.window;
    else globalThis.window = previousWindow;
  }
}

test("server settings render password-only credential controls, privacy, reserve and checked metadata", async () => {
  const html = await renderRecognition(SourceFields, {
    prepare: async (bindings) => {
      await bindings.controller.loadStatus();
      await bindings.controller.check();
    },
  });
  assert.match(html, /type="password"/);
  assert.match(html, /Сохранить токен/);
  assert.match(html, /Удалить токен/);
  assert.match(html, /Kari/);
  assert.match(html, /Аудио будет отправляться/);
  assert.match(html, /Сервер ответил/);
  assert.match(html, /Demo GPU/);
  assert.match(html, /API 2/);
  assert.match(html, /Проверка не сохраняет токен/);
  assert.match(html, /Если сервер недоступен/);
  const demo = await renderRecognition(SourceFields, { native: false });
  assert.match(demo, /браузерный пример/);
});

test("typed connection failure is visible as an alert without leaking transport details", async () => {
  const html = await renderRecognition(SourceFields, {
    port: sourcePort({
      test: async () => ({ kind: "error", error: "unauthorized" }),
    }),
    prepare: (bindings) => bindings.controller.check(),
  });
  assert.match(html, /role="alert"/);
  assert.match(html, /Сервер не принял токен/);
  assert.doesNotMatch(html, /\n\s+at |Authorization:|Bearer /);
});

test("remote model manager is read-only and remote recognition hides local GPU and thread settings", async () => {
  const models = await renderRecognition(RemoteModels, {
    props: { remote: true, server: remoteServer() },
  });
  assert.match(models, /Remote Whisper/);
  assert.match(models, /только для просмотра/);
  assert.doesNotMatch(models, /<button|<progress/);
  const audio = await renderRecognition(AudioFields, {
    props: { scope: "recognition" },
  });
  assert.match(audio, /Локальная видеокарта не используется/);
  assert.doesNotMatch(audio, /id="(?:acceleration|whisper-threads)"/);
  assert.ok(sectionKeys.audio.includes("recognitionSource"));
});
