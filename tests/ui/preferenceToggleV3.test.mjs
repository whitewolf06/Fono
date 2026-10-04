import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer } from "vite";
import { createSSRApp, h } from "vue";
import {
  createToggleHarness,
  flushChanges,
} from "./preferenceToggleHarness.mjs";
import { renderToString } from "vue/server-renderer";
import { WlConfig, WlToastService } from "@whitelife-core/ui-kit";

let server,
  PreferenceToggle,
  mountControlledToggle,
  workspaceKey,
  interactionKey;

before(async () => {
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    cacheDir: "node_modules/.vite/preference-toggle-tests",
    optimizeDeps: { noDiscovery: true, include: [] },
    server: { hmr: false, middlewareMode: true },
  });
  ({ default: PreferenceToggle } = await server.ssrLoadModule(
    "/src/v3/features/preferences/presentation/PreferenceToggle.vue",
  ));
  ({ workspaceKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/workspace.ts",
  ));
  ({ interactionKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/interaction.ts",
  ));
  mountControlledToggle = await createToggleHarness(
    server,
    workspaceKey,
    interactionKey,
  );
});

after(async () => await server?.close());

async function renderToggle({ enabled, pending = false, ...props }) {
  const name = props.name || "serviceEnabled";
  const app = createSSRApp({
    render: () =>
      h(PreferenceToggle, {
        name: "serviceEnabled",
        label: "API-сервис",
        ...props,
      }),
  });
  app.use(WlConfig);
  app.use(WlToastService);
  app.provide(workspaceKey, {
    state: {
      preferences: { [name]: enabled },
      pending: { [name]: pending },
    },
  });
  app.provide(interactionKey, { confirm: async () => false });
  return renderToString(app);
}

function inputAttributes(html) {
  const input = html.match(/<input\b([^>]*)>/);
  assert.ok(input, "The real WhiteUI switch must render its input");
  return input[1];
}

function assertChecked(html, expected) {
  const input = inputAttributes(html);
  assert.match(input, /role="switch"/);
  assert.match(input, /aria-label="API-сервис"/);
  assert.match(input, new RegExp(`aria-checked="${expected}"`));
  assert.equal(/\bchecked(?:\s|$)/.test(input), expected);
  assert.equal(/\bwl-switch--md is-checked\b/.test(html), expected);
}

test("absent optional effectiveValue preserves enabled state of the compiled Vue component", async () => {
  assertChecked(await renderToggle({ enabled: true }), true);
  assertChecked(await renderToggle({ enabled: false }), false);
});

test("undefined effectiveValue falls back to preferences while explicit Boolean overrides remain authoritative", async () => {
  assertChecked(
    await renderToggle({ enabled: true, effectiveValue: undefined }),
    true,
  );
  assertChecked(
    await renderToggle({ enabled: true, effectiveValue: false }),
    false,
  );
  assertChecked(
    await renderToggle({ enabled: false, effectiveValue: true }),
    true,
  );
});

test("compact switches retain the persisted state and accessible label", async () => {
  const html = await renderToggle({ enabled: true, compact: true });
  assertChecked(html, true);
  assert.match(html, /class="toggle-row is-compact"/);
});

test("pending save disables the switch and announces activity without losing enabled state", async () => {
  const html = await renderToggle({ enabled: true, pending: true });
  assertChecked(html, true);
  assert.match(inputAttributes(html), /\bdisabled(?:\s|$)/);
  assert.match(html, /aria-busy="true"/);
  assert.match(html, /role="status">Сохранение<\/span>/);
});

test("explicit disabled input retains its value; idle input remains interactive", async () => {
  const disabled = await renderToggle({ enabled: true, disabled: true });
  assertChecked(disabled, true);
  assert.match(inputAttributes(disabled), /\bdisabled(?:\s|$)/);
  const enabled = await renderToggle({ enabled: false });
  assert.doesNotMatch(inputAttributes(enabled), /\bdisabled(?:\s|$)/);
  assert.doesNotMatch(enabled, /aria-busy="true"/);
});

test("cancelled trainer consent restores native checked, keeps focus and blocks another pending change", async () => {
  let answer,
    confirmations = 0;
  const control = mountControlledToggle(
    "trainerEnabled",
    { preferences: { trainerEnabled: false, analyticsConsent: false } },
    () => {
      confirmations++;
      return new Promise((resolve) => (answer = resolve));
    },
  );
  try {
    control.assertState(false);
    await control.change(true);
    await flushChanges();
    control.assertState(false);
    assert.equal(control.input.props.disabled, true);
    await control.change(true);
    assert.equal(confirmations, 1);
    answer(false);
    await flushChanges();
    control.assertState(false);
    assert.equal(control.state.preferences.trainerEnabled, false);
    assert.equal(control.input.props.disabled, false);
  } finally {
    control.dispose();
  }
});

test("unavailable wake stays disabled and unchecked even for a verified profile", async () => {
  const control = mountControlledToggle(
    "wakeEnabled",
    {
      preferences: { wakeEnabled: false },
      wakeSetup: { verified: true },
    },
    async () => false,
  );
  try {
    await control.change(true);
    await flushChanges();
    control.assertState(false);
    assert.equal(control.state.preferences.wakeEnabled, false);
    assert.equal(control.input.props.disabled, true);
  } finally {
    control.dispose();
  }
});

test("unavailable wake renders unchecked even if an old UI snapshot still says enabled", async () => {
  const html = await renderToggle({
    enabled: true,
    name: "wakeEnabled",
    label: "Пробуждение",
  });
  assert.match(inputAttributes(html), /aria-checked="false"/);
  assert.match(inputAttributes(html), /\bdisabled(?:\s|$)/);
  assert.match(html, /Временно недоступно/);
});

test("activation form disables wake fields and setup but preserves both hotkeys", async () => {
  const { default: ActivationFields } = await server.ssrLoadModule(
    "/src/v3/features/preferences/presentation/ActivationFields.vue",
  );
  const { defaults } = await server.ssrLoadModule(
    "/src/v3/features/preferences/domain/preferences.ts",
  );
  const app = createSSRApp({
    render: () =>
      h(ActivationFields, { modelValue: { ...defaults }, advanced: true }),
  });
  app.use(WlConfig);
  app.use(WlToastService);
  app.provide(workspaceKey, {
    native: false,
    wake: {},
    state: { preferences: { ...defaults }, phase: "idle", pending: {} },
  });
  app.provide(interactionKey, { confirm: async () => false });
  const html = await renderToString(app);
  assert.match(html, /Временно недоступно/);
  assert.match(html, /Начинайте диктовку кнопкой или горячей клавишей/);
  const input = (id) =>
    html.match(new RegExp(`<input\\b[^>]*id="${id}"[^>]*>`))?.[0];
  assert.ok(input("wakePhrase"));
  assert.match(input("wakePhrase"), /\bdisabled(?:\s|>)/);
  for (const id of ["hotkey", "commandHotkey"]) {
    assert.ok(input(id));
    assert.doesNotMatch(input(id), /\bdisabled(?:\s|>)/);
  }
  assert.doesNotMatch(html, />Включить пробуждение</);
});

test("accepted trainer consent commits both preferences and updates the same controlled input", async () => {
  let answer;
  const control = mountControlledToggle(
    "trainerEnabled",
    { preferences: { trainerEnabled: false, analyticsConsent: false } },
    () => new Promise((resolve) => (answer = resolve)),
  );
  try {
    await control.change(true);
    await flushChanges();
    control.assertState(false);
    assert.equal(control.input.props.disabled, true);
    answer(true);
    await flushChanges();
    control.assertState(true);
    assert.equal(control.state.preferences.trainerEnabled, true);
    assert.equal(control.state.preferences.analyticsConsent, true);
    assert.equal(control.input.props.disabled, false);
  } finally {
    control.dispose();
  }
});

test("cancelled analytics removal retains native checked and the existing consent", async () => {
  let answer;
  const control = mountControlledToggle(
    "analyticsConsent",
    { preferences: { trainerEnabled: true, analyticsConsent: true } },
    () => new Promise((resolve) => (answer = resolve)),
  );
  try {
    await control.change(false);
    await flushChanges();
    control.assertState(true);
    answer(false);
    await flushChanges();
    control.assertState(true);
    assert.equal(control.state.preferences.analyticsConsent, true);
    assert.equal(control.state.preferences.trainerEnabled, true);
  } finally {
    control.dispose();
  }
});
