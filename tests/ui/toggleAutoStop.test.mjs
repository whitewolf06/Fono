import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer } from "vite";
import { createSSRApp, h } from "vue";
import { renderToString } from "vue/server-renderer";
import { WlConfig, WlToastService } from "@whitelife-core/ui-kit";

let server,
  defaults,
  rawDefaults,
  preferences,
  mapping,
  browser,
  policy,
  locale,
  createWorkspace,
  createDictation,
  nativeDictation,
  TriggerFields,
  workspaceKey,
  interactionKey,
  createInteraction;
const storage = new Map();
const flush = () => new Promise((resolve) => setImmediate(resolve));
before(async () => {
  globalThis.localStorage = {
    getItem: (k) => storage.get(k) ?? null,
    setItem: (k, v) => storage.set(k, v),
  };
  globalThis.window = { addEventListener() {}, removeEventListener() {} };
  server = await createServer({
    appType: "custom",
    server: { middlewareMode: true, hmr: false },
  });
  const load = (p) => server.ssrLoadModule("/src/v3/" + p);
  preferences = await load("features/preferences/domain/preferences.ts");
  ({ defaults } = preferences);
  ({ DEFAULT_SETTINGS: rawDefaults } = await load(
    "shared/infrastructure/native/ipcTypes.ts",
  ));
  mapping = await load("shared/infrastructure/native/mapping.ts");
  browser = await load("shared/infrastructure/browser.ts");
  policy = await load("shared/domain/toggleAutoStop.ts");
  locale = await load("shared/domain/locale.ts");
  ({ createMockWorkspace: createWorkspace } = await load(
    "app/mockWorkspace.ts",
  ));
  ({ createDictationPort: createDictation } = await load(
    "features/dictation/infrastructure/mockDictation.ts",
  ));
  ({ nativeDictation } = await load(
    "shared/infrastructure/native/dictation.ts",
  ));
  ({ default: TriggerFields } = await load(
    "features/preferences/presentation/TriggerFields.vue",
  ));
  ({ workspaceKey } = await load("shared/application/workspace.ts"));
  ({ interactionKey, createInteraction } = await load(
    "shared/application/interaction.ts",
  ));
});
after(async () => {
  await server?.close();
  delete globalThis.localStorage;
  delete globalThis.window;
});

test("legacy settings default off; 2-10 seconds round-trip separately from wake silence and speech language", () => {
  assert.equal(defaults.toggleAutoStopMs, 0);
  assert.equal(
    mapping.preferencesFromNative(rawDefaults, []).toggleAutoStopMs,
    0,
  );
  for (const timeout of [0, 2000, 2500, 10000]) {
    assert.equal(policy.validToggleAutoStop(timeout), true);
    assert.equal(
      preferences.validatePreferences({
        ...defaults,
        toggleAutoStopMs: timeout,
      }),
      null,
    );
    const native = mapping.applyPreferences(
      { ...rawDefaults, language: "en", wake_dictation_silence_ms: 1600 },
      { toggleAutoStopMs: timeout },
      [],
    );
    assert.equal(native.toggle_auto_stop_ms, timeout);
    assert.equal(native.wake_dictation_silence_ms, 1600);
    assert.equal(native.language, "en");
    assert.equal(
      mapping.preferencesFromNative(native, []).toggleAutoStopMs,
      timeout,
    );
  }
  for (const value of [-1, 1, 1999, 10001, NaN, Infinity, 2000.5])
    assert.ok(
      preferences.validatePreferences({ ...defaults, toggleAutoStopMs: value }),
    );
  assert.ok(preferences.sectionKeys.activation.includes("toggleAutoStopMs"));
  assert.ok(preferences.quickKeys.hotkey.includes("toggleAutoStopMs"));
});

test("browser stores timing without texts and safely disables an invalid saved timeout", () => {
  browser.persistPreferences({
    ...defaults,
    toggleAutoStopMs: 3000,
    instruction: "PRIVATE",
    original: "PRIVATE TEXT",
  });
  assert.equal(browser.readPreferences().toggleAutoStopMs, 3000);
  assert.ok(![...storage.values()].join().includes("PRIVATE"));
  storage.set(
    "fono-v3-demo-preferences-v1",
    JSON.stringify({ toggleAutoStopMs: 90000 }),
  );
  assert.equal(browser.readPreferences().toggleAutoStopMs, 0);
  storage.clear();
});

test("countdown rejects retired operations and malformed status; current speech resets it", () => {
  const status = {
    operation_id: 5,
    phase: "silence",
    timeout_ms: 3000,
    remaining_ms: 1200,
  };
  assert.deepEqual(policy.toggleAutoStopCountdown(status, 5), {
    timeout_ms: 3000,
    remaining_ms: 1200,
    speaking: false,
  });
  for (const change of [
    { operation_id: 4 },
    { phase: "inactive" },
    { phase: "unavailable" },
    { timeout_ms: 10001 },
    { remaining_ms: NaN },
  ])
    assert.equal(
      policy.toggleAutoStopCountdown({ ...status, ...change }, 5),
      undefined,
    );
  assert.equal(
    policy.toggleAutoStopCountdown(
      { ...status, phase: "listening", remaining_ms: 3000 },
      5,
    ).speaking,
    true,
  );
});

function mockHarness(noSpeech = false, mode = "toggle", timeout = 2000) {
  const workspace = createWorkspace();
  const state = workspace.state;
  state.scenario = noSpeech ? "no-speech" : "normal";
  state.preferences.hotkeyMode = mode;
  state.preferences.toggleAutoStopMs = timeout;
  state.preferences.processingEnabled = false;
  const callbacks = [],
    delays = [];
  const port = createDictation(
    state,
    async (ms) => {
      delays.push(ms);
    },
    {
      setInterval(cb) {
        callbacks.push(cb);
        return callbacks.length;
      },
      clearInterval() {},
    },
  );
  return {
    workspace,
    state,
    port,
    callbacks,
    delays,
    tick(n) {
      const callback = callbacks.at(-1);
      for (let i = 0; i < n; i++) callback();
    },
    dispose() {
      port.dispose();
      workspace.dispose();
    },
  };
}

test("toggle demo auto-stops once after speech; hold and disabled modes keep recording", async () => {
  const enabled = mockHarness();
  try {
    enabled.port.start();
    const count = enabled.state.history.length;
    enabled.tick(119);
    assert.equal(enabled.state.phase, "silence");
    assert.equal(Math.round(enabled.state.countdown.remaining_ms), 100);
    enabled.tick(1);
    enabled.tick(20);
    await flush();
    assert.equal(enabled.state.phase, "done");
    assert.equal(enabled.state.history.length, count + 1);
    assert.equal(enabled.delays.filter((ms) => ms === 800).length, 1);
  } finally {
    enabled.dispose();
  }
  for (const [mode, timeout] of [
    ["hold", 2000],
    ["toggle", 0],
  ]) {
    const harness = mockHarness(false, mode, timeout);
    try {
      harness.port.start();
      harness.tick(200);
      assert.equal(harness.state.phase, "listening");
      assert.equal(harness.delays.length, 0);
    } finally {
      harness.dispose();
    }
  }
});

test("initial silence reaches NoSpeech without STT, processing, history or replacing last text", () => {
  const h = mockHarness(true);
  try {
    const before = JSON.stringify({
      history: h.state.history,
      last: h.state.last,
    });
    h.port.start();
    h.tick(20);
    assert.equal(h.state.phase, "idle");
    assert.equal(h.state.dictationOutcome, "no_speech");
    assert.equal(h.delays.length, 0);
    assert.equal(
      JSON.stringify({ history: h.state.history, last: h.state.last }),
      before,
    );
  } finally {
    h.dispose();
  }
});

test("an old timer after Cancel or restart cannot stop or advance its successor", () => {
  const h = mockHarness();
  try {
    h.port.start();
    const old = h.callbacks.at(-1);
    h.port.cancel();
    h.port.start();
    for (let i = 0; i < 200; i++) old();
    assert.equal(h.state.elapsed, 0);
    assert.equal(h.state.phase, "listening");
    assert.equal(h.delays.length, 0);
    h.tick(10);
    assert.equal(h.state.elapsed, 1);
  } finally {
    h.dispose();
  }
});

test("native snapshot and meter use the same countdown; late meters cannot affect a new recording", () => {
  const workspace = createWorkspace(),
    state = workspace.state;
  const dictation = nativeDictation({
    state,
    readHistory: async () => {},
    report: (error) => {
      throw error;
    },
  });
  const snapshot = (id, auto_stop) => ({
    operation_id: id,
    state: "listening",
    level: 0,
    last: null,
    source: "hotkey",
    auto_stop,
  });
  try {
    dictation.snapshot(
      snapshot(21, {
        operation_id: 21,
        phase: "silence",
        timeout_ms: 2000,
        remaining_ms: 500,
      }),
    );
    assert.equal(state.phase, "silence");
    assert.equal(state.countdown.remaining_ms, 500);
    dictation.level({
      operation_id: 21,
      level: 0,
      elapsed_ms: 1000,
      auto_stop: {
        operation_id: 21,
        phase: "listening",
        timeout_ms: 2000,
        remaining_ms: 2000,
      },
    });
    assert.equal(state.phase, "listening");
    assert.equal(state.countdown.speaking, true);
    dictation.snapshot(snapshot(22));
    dictation.level({
      operation_id: 21,
      level: 1,
      elapsed_ms: 99999,
      auto_stop: {
        operation_id: 21,
        phase: "stopping",
        timeout_ms: 2000,
        remaining_ms: 0,
      },
    });
    assert.equal(state.phase, "listening");
    assert.equal(state.countdown, undefined);
    dictation.outcome({ operation_id: 22, outcome: "cancelled" });
    dictation.level({
      operation_id: 22,
      level: 1,
      elapsed_ms: 1000,
      auto_stop: {
        operation_id: 22,
        phase: "silence",
        timeout_ms: 2000,
        remaining_ms: 0,
      },
    });
    assert.equal(state.phase, "cancelled");
  } finally {
    dictation.dispose();
    workspace.dispose();
  }
});

test("RU/EN toggle controls show the option only for ordinary toggle and disable it during capture", async () => {
  for (const language of ["ru", "en"]) {
    locale.bindMessageLocale(() => language);
    const workspace = createWorkspace();
    async function render(mode, scope = "hotkey", active = false) {
      workspace.state.preferences.hotkeyMode = mode;
      const app = createSSRApp({
        render: () =>
          h(TriggerFields, {
            modelValue: workspace.state.preferences,
            scope,
            active,
          }),
      });
      app.use(WlConfig);
      app.use(WlToastService);
      app.provide(workspaceKey, workspace);
      app.provide(interactionKey, createInteraction());
      return renderToString(app);
    }
    try {
      assert.ok(!(await render("hold")).includes('id="toggleAutoStopMs"'));
      const html = await render("toggle");
      assert.ok(
        html.includes(
          language === "ru"
            ? "Завершать запись по тишине"
            : "Stop recording after silence",
        ),
      );
      assert.ok(
        !(await render("toggle", "command-hotkey")).includes(
          'id="toggleAutoStopMs"',
        ),
      );
      assert.match(
        await render("toggle", "hotkey", true),
        /id="toggleAutoStopMs"[^>]*disabled|disabled[^>]*id="toggleAutoStopMs"/,
      );
    } finally {
      workspace.dispose();
    }
  }
  locale.bindMessageLocale(() => "ru");
});
