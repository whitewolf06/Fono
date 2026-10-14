import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer } from "vite";
import { createSSRApp, h } from "vue";
import { renderToString } from "vue/server-renderer";
import { WlConfig, WlToastService } from "@whitelife-core/ui-kit";

const form =
  "https://feedback.example/feedback/submit/dfd4f4c5-4c15-48d2-91e1-0f41be11d7ea";
let server, channel, browser, native, FeedbackCard, workspaceKey;
let opened, calls;
before(async () => {
  globalThis.window = {
    open: (...args) => opened.push(args),
    __TAURI_INTERNALS__: {
      invoke: async (command, args) => calls.push([command, args]),
    },
  };
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    cacheDir: "node_modules/.vite/feedback-channel-tests",
    optimizeDeps: { noDiscovery: true, include: [] },
    server: { hmr: false, middlewareMode: true },
  });
  channel = await server.ssrLoadModule(
    "/src/v3/features/feedback/domain/channel.ts",
  );
  browser = await server.ssrLoadModule(
    "/src/v3/features/feedback/infrastructure/browserChannel.ts",
  );
  native = await server.ssrLoadModule(
    "/src/v3/features/feedback/infrastructure/nativeChannel.ts",
  );
  ({ default: FeedbackCard } = await server.ssrLoadModule(
    "/src/v3/features/feedback/presentation/FeedbackCard.vue",
  ));
  ({ workspaceKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/workspace.ts",
  ));
});
after(async () => {
  await server?.close();
  delete globalThis.window;
});

test("channel rejects arbitrary URLs, credentials, queries and fragments", () => {
  assert.equal(channel.validateFeedbackUrl(form)?.href, form);
  for (const value of [
    "",
    "javascript:alert(1)",
    "file:///feedback/submit/dfd4f4c5-4c15-48d2-91e1-0f41be11d7ea",
    form + "?",
    form + "#",
    form + "?token=secret",
    form + "#private",
    form.replace("https://", "https://user:secret@"),
    form.replace("/feedback/submit/", "/private/"),
    form.replace("/feedback/submit/", "/ignored/../feedback/submit/"),
    form.replace("dfd4f4c5-4c15-48d2-91e1-0f41be11d7ea", "invalid"),
    form + "\\private",
  ])
    assert.equal(channel.validateFeedbackUrl(value, true), null, value);
});

test("debug HTTP requires explicit loopback and release disallows it", () => {
  const local = form.replace(
    "https://feedback.example",
    "http://127.0.0.1:5380",
  );
  assert.ok(channel.validateFeedbackUrl(local, true));
  assert.equal(channel.validateFeedbackUrl(local, false), null);
  assert.equal(
    channel.validateFeedbackUrl(form.replace("https:", "http:"), true),
    null,
  );
});

test("context is absent by default and only the three safe fields can be added", () => {
  assert.equal(channel.feedbackUrl(form), form);
  const safe = channel.feedbackContext("0.6.17", true);
  const input = { ...safe, apiKey: "PRIVATE", logs: "PRIVATE" };
  const url = new URL(channel.feedbackUrl(form, input));
  assert.equal(url.search, "");
  assert.deepEqual(
    [...new URLSearchParams(url.hash.slice(1))],
    [
      ["version", "0.6.17"],
      ["platform", "windows"],
      ["route", "/settings/diagnostics"],
    ],
  );
  assert.ok(!url.href.includes("PRIVATE"));
});

test("disabled browser and native channels perform no action", async () => {
  opened = [];
  calls = [];
  const browserPort = browser.createBrowserFeedbackChannel("");
  const nativePort = native.createNativeFeedbackChannel("");
  assert.equal(browserPort.available, false);
  assert.equal(nativePort.available, false);
  await assert.rejects(browserPort.open(false), /не настроена/);
  await assert.rejects(nativePort.open(true), /не настроена/);
  assert.deepEqual(opened, []);
  assert.deepEqual(calls, []);
});

test("browser adapter opens only the configured form and opt-in fragment", async () => {
  opened = [];
  const port = browser.createBrowserFeedbackChannel(form);
  await port.open(false);
  assert.deepEqual(opened[0], [form, "_blank", "noopener,noreferrer"]);
  await port.open(true);
  const url = new URL(opened[1][0]);
  assert.equal(url.search, "");
  assert.equal(
    new URLSearchParams(url.hash.slice(1)).get("platform"),
    "browser-mock",
  );
});

test("native IPC receives only consent and never URL, context or credentials", async () => {
  calls = [];
  const port = native.createNativeFeedbackChannel(form);
  await port.open(false);
  await port.open(true);
  assert.deepEqual(calls, [
    ["open_feedback_form", { includeContext: false }],
    ["open_feedback_form", { includeContext: true }],
  ]);
});

test("disabled form has unavailable notice, disabled button and unchecked consent", async () => {
  const app = createSSRApp({ render: () => h(FeedbackCard) });
  app.use(WlConfig);
  app.use(WlToastService);
  app.provide(workspaceKey, {
    feedback: browser.createBrowserFeedbackChannel(""),
  });
  const html = await renderToString(app);
  assert.match(html, /Форма обратной связи пока не настроена/);
  assert.match(html, /<button[^>]*disabled/);
  assert.ok(!/\schecked(?:=|\s|>)/.test(html));
  assert.match(html, /aria-checked="false"/);
  assert.match(html, /Добавить версию и раздел Fono/);
});
