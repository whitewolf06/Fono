import assert from "node:assert/strict";
import fs from "node:fs";
import { before, after, afterEach, test } from "node:test";
import { createServer } from "vite";
import { createSSRApp, h, reactive, nextTick } from "vue";
import { renderToString } from "vue/server-renderer";
import { createRouter, createMemoryHistory } from "vue-router";
import { WlConfig, WlToastService } from "@whitelife-core/ui-kit";

const ru = JSON.parse(
  fs.readFileSync("src/v3/shared/domain/locales/ru.json", "utf8"),
);
const en = JSON.parse(
  fs.readFileSync("src/v3/shared/domain/locales/en.json", "utf8"),
);
const keyFor = (source) => Object.keys(ru).find((key) => ru[key] === source);
let server,
  locale,
  messages,
  defaults,
  mapping,
  rawDefaults,
  createWorkspace,
  workspaceKey,
  interactionKey,
  createInteraction,
  localization,
  storage;
before(async () => {
  storage = new Map();
  globalThis.localStorage = {
    getItem: (key) => storage.get(key) ?? null,
    setItem: (key, value) => storage.set(key, value),
  };
  globalThis.window = { addEventListener() {}, removeEventListener() {} };
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { hmr: false, middlewareMode: true },
  });
  locale = await server.ssrLoadModule("/src/v3/shared/domain/locale.ts");
  messages = await server.ssrLoadModule("/src/v3/shared/domain/messages.ts");
  ({ defaults } = await server.ssrLoadModule(
    "/src/v3/features/preferences/domain/preferences.ts",
  ));
  mapping = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/mapping.ts",
  );
  ({ DEFAULT_SETTINGS: rawDefaults } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/ipcTypes.ts",
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
  localization = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/localization.ts",
  );
});
afterEach(() => {
  locale.bindMessageLocale(() => "ru");
  storage.clear();
});
after(async () => {
  await server?.close();
  delete globalThis.localStorage;
  delete globalThis.window;
});

test("UI locale migration uses RU Windows, falls back to EN, and respects an explicit choice", () => {
  for (const system of ["ru-RU", "ru_RU.UTF-8", "RU"])
    assert.equal(locale.resolveUiLocale("auto", system), "ru");
  for (const system of ["en-US", "de-DE", "zh-CN", "", "unknown"])
    assert.equal(locale.resolveUiLocale("auto", system), "en");
  assert.equal(locale.resolveUiLocale("en", "ru-RU"), "en");
  assert.equal(locale.resolveUiLocale("ru", "en-US"), "ru");
  assert.equal(locale.resolveUiLocale("future-locale", "ru-RU"), "ru");
  assert.equal(locale.availableUiLanguage(undefined), "auto");
  assert.equal(
    mapping.preferencesFromNative(rawDefaults, []).uiLanguage,
    "auto",
  );
});

test("English has every Russian key and matching parameters, including the closed native error catalogue", () => {
  assert.deepEqual(Object.keys(en).sort(), Object.keys(ru).sort());
  const parameters = (value) =>
    [...value.matchAll(/\{([a-zA-Z][a-zA-Z0-9_]*)\}/gu)]
      .map((m) => m[1])
      .sort();
  for (const key of Object.keys(ru)) {
    assert.ok(en[key].trim(), key);
    assert.deepEqual(parameters(en[key]), parameters(ru[key]), key);
  }
  const errors = JSON.parse(
    fs.readFileSync(
      "src/v3/shared/infrastructure/native/userErrorCatalog.json",
      "utf8",
    ),
  );
  for (const source of [
    ...Object.values(errors.messages),
    ...errors.details,
    ...Object.values(errors.templates),
  ])
    assert.ok(keyFor(source), source);
});

test("locale changes translate authored labels while interpolation keeps user content and literal snippet variables", () => {
  const current = reactive({ locale: "ru" });
  locale.bindMessageLocale(() => current.locale);
  const key = keyFor("Главная");
  assert.equal(messages.message(key), "Главная");
  current.locale = "en";
  assert.equal(messages.message(key), "Home");
  assert.equal(
    messages.translateAuthored("Пользовательский текст без перевода"),
    "Пользовательский текст без перевода",
  );
  const description = keyFor(
    Object.values(ru).find(
      (value) => value.includes("{date}") && value.includes("{clipboard}"),
    ),
  );
  const rendered = messages.message(description);
  assert.ok(
    rendered.includes("{date}") &&
      rendered.includes("{time}") &&
      rendered.includes("{clipboard}"),
  );
  const privateText = "Личный текст {date} <script>private</script>";
  assert.equal(
    messages.message(keyFor("Доступна {v0}"), { v0: privateText }),
    privateText + " available",
  );
  assert.throws(
    () => messages.message(keyFor("Доступна {v0}")),
    (error) => error.code === "missing_parameter",
  );
  assert.throws(
    () => messages.message("missing.test.key"),
    (error) => error.code === "missing_message",
  );
  assert.equal(
    messages.createMessageDictionary({ test: "Текст будущего ключа" })("test"),
    "Текст будущего ключа",
  );
});

test("saving UI language leaves recognition, translation, custom instructions and secrets unchanged", () => {
  const before = {
    ...structuredClone(rawDefaults),
    language: "ru",
    processing_target_language: "de",
    llm_api_key: "PRIVATE",
    clean_prompt: "PRIVATE инструкция",
  };
  const next = mapping.applyPreferences(before, { uiLanguage: "en" }, []);
  assert.equal(next.ui_language, "en");
  assert.equal(next.language, "ru");
  assert.equal(next.processing_target_language, "de");
  assert.equal(next.clean_prompt, before.clean_prompt);
  assert.equal(next.llm_api_key, "PRIVATE");
  assert.equal(before.ui_language, undefined);
  assert.equal(mapping.preferencesFromNative(next, []).uiLanguage, "en");
});

test("browser remembers only the locale preference and normalizes unsupported saved choices", async () => {
  const browser = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/browser.ts",
  );
  browser.persistPreferences({
    ...defaults,
    uiLanguage: "en",
    language: "ru",
    instruction: "PRIVATE",
    transcript: "PRIVATE TEXT",
  });
  assert.equal(browser.readPreferences().uiLanguage, "en");
  assert.equal(browser.readPreferences().language, "ru");
  assert.ok(![...storage.values()].join().includes("PRIVATE"));
  storage.set(
    "fono-v3-demo-preferences-v1",
    JSON.stringify({ uiLanguage: "future" }),
  );
  assert.equal(browser.readPreferences().uiLanguage, "auto");
});

test("English errors keep the same codes and never expose untrusted native details", async () => {
  const { NativeIpcError } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/nativeErrors.ts",
  );
  locale.bindMessageLocale(() => "en");
  const error = new NativeIpcError({
    code: "config",
    params: { detail: "PRIVATE C:/secret/token" },
  });
  assert.equal(error.code, "config");
  assert.match(error.message, /Could not apply settings/);
  assert.ok(!error.message.includes("PRIVATE"));
  const missing = new NativeIpcError({
    code: "model_not_selected",
    params: {},
  });
  assert.match(missing.message, /Select a recognition model/);
  const backend = new NativeIpcError({
    code: "backend_unavailable",
    params: { backend: "cpu" },
  });
  assert.match(backend.message, /CPU/);
  assert.doesNotMatch(backend.message, /[А-Яа-яЁё]/);
});

test("changing UI locale preserves analysis, phrase identities, source texts and trainer fingerprints", async () => {
  const { analyze } = await server.ssrLoadModule(
    "/src/v3/features/trainer/domain/analysis.ts",
  );
  const { buildTrainerSnapshot } = await server.ssrLoadModule(
    "/src/v3/features/trainer/domain/period.ts",
  );
  const { supportedCommands } = await server.ssrLoadModule(
    "/src/v3/features/commands/domain/catalog.ts",
  );
  const { chartCategories } = await server.ssrLoadModule(
    "/src/v3/features/trainer/application/chart.ts",
  );
  const now = Date.parse("2026-10-21T12:00:00Z");
  const entry = {
    id: "test",
    createdAt: new Date(now - 1000).toISOString(),
    original: "Ну, как бы, я я хотел сказать, вернее, хотел уточнить.",
    text: "PRIVATE исходный текст",
    duration: 10,
    analyticsAllowed: true,
  };
  const preferences = {
    ...defaults,
    analyticsConsent: true,
    historyEnabled: true,
    trainerEnabled: true,
  };
  locale.bindMessageLocale(() => "ru");
  const before = {
    analysis: analyze(entry),
    report: buildTrainerSnapshot([entry], preferences, 7, now),
    phrases: supportedCommands.map((x) => x.phrase),
    categories: chartCategories.map((x) => x.title),
  };
  locale.bindMessageLocale(() => "en");
  const after = {
    analysis: analyze(entry),
    report: buildTrainerSnapshot([entry], preferences, 7, now),
    phrases: supportedCommands.map((x) => x.phrase),
    categories: chartCategories.map((x) => x.title),
  };
  assert.deepEqual(after, before);
  assert.equal(entry.text, "PRIVATE исходный текст");
  const nativeEntry = mapping.historyFromNative({
    id: "native",
    text: entry.text,
    original_text: entry.original,
    created_at: entry.createdAt,
    analysis_status: "ready",
    analysis: {
      filler_count: 2,
      repetition_count: 1,
      self_correction_count: 1,
      unfinished_count: 0,
      findings: [],
    },
  });
  assert.deepEqual(
    nativeEntry.findings.map((f) => f.title),
    before.categories,
  );
  assert.deepEqual(
    analyze(nativeEntry).map((f) => f.count),
    [2, 1, 1, 0],
  );
});

test("cached authored status messages switch both ways without translating unknown status content", () => {
  locale.bindMessageLocale(() => "en");
  const cached = messages.translateAuthored(
    "Пакет CUDA установлен и проверен.",
  );
  assert.equal(cached, "The CUDA package is installed and verified.");
  locale.bindMessageLocale(() => "ru");
  assert.equal(
    messages.translateAuthored(cached),
    "Пакет CUDA установлен и проверен.",
  );
  assert.equal(
    messages.translateAuthored("PRIVATE user status"),
    "PRIVATE user status",
  );
});

test("language changes update WhiteUI, document language and labels without replacing the runtime", async () => {
  const previousDocument = globalThis.document;
  globalThis.document = { documentElement: { lang: "" } };
  const preferences = reactive({ uiLanguage: "auto" });
  localization.initializeLocalization("auto", "ru-RU");
  const release = localization.followUiLanguage(() => preferences.uiLanguage);
  try {
    assert.equal(messages.message(keyFor("Настройки")), "Настройки");
    preferences.uiLanguage = "en";
    await nextTick();
    assert.equal(messages.message(keyFor("Настройки")), "Settings");
    assert.equal(globalThis.document.documentElement.lang, "en");
    assert.equal(localization.whiteUiLocale.monthNames[0], "January");
    assert.equal(localization.whiteUiLocale.reject, "Cancel");
    preferences.uiLanguage = "ru";
    await nextTick();
    assert.equal(globalThis.document.documentElement.lang, "ru");
  } finally {
    release();
    globalThis.document = previousDocument;
  }
});

test("RU and EN render the main pages, settings, overlay and data boundaries through the same components", async () => {
  const cases = [
    [
      "/src/v3/features/dictation/presentation/HomePage.vue",
      "/",
      "Голосовой ввод",
      "Voice input",
    ],
    [
      "/src/v3/features/files/presentation/FilesPage.vue",
      "/files",
      ru["ui.9088ca9c0d"],
      en["ui.9088ca9c0d"],
    ],
    [
      "/src/v3/features/preferences/presentation/SettingsPage.vue",
      "/settings/general",
      "Язык интерфейса",
      "Interface language",
    ],
    [
      "/src/v3/features/commands/presentation/CommandsPage.vue",
      "/commands",
      "Голосовые команды",
      "Voice commands",
    ],
    [
      "/src/v3/features/trainer/presentation/TrainerPage.vue",
      "/trainer",
      "Речевые привычки",
      "Speech habits",
    ],
    [
      "/src/v3/features/overlay/presentation/OverlayPreview.vue",
      "/overlay",
      "Выбрать язык распознавания",
      "Choose recognition language",
    ],
  ];
  for (const language of ["ru", "en"]) {
    locale.bindMessageLocale(() => language);
    for (const [file, path, russian, english] of cases) {
      const { default: Component } = await server.ssrLoadModule(file);
      const workspace = createWorkspace();
      workspace.state.preferences.hotkeyMode = "toggle";
      const router = createRouter({
        history: createMemoryHistory(),
        routes: [
          { path: "/:pathMatch(.*)*", component: { render: () => null } },
        ],
      });
      await router.push(path);
      await router.isReady();
      const app = createSSRApp({
        render: () =>
          h(Component, {
            preferences: workspace.state.preferences,
            phase: "listening",
            interactive: true,
          }),
      });
      app.use(router);
      app.use(WlConfig, { locale: localization.whiteUiLocale });
      app.use(WlToastService);
      app.provide(workspaceKey, workspace);
      app.provide(interactionKey, createInteraction());
      try {
        const html = await renderToString(app);
        assert.ok(
          html.includes(language === "ru" ? russian : english),
          `${language}: ${file}`,
        );
        assert.ok(!html.includes("missing_message"));
      } finally {
        workspace.dispose();
      }
    }
  }
});
