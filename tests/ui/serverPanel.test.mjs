import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";
import { createSSRApp, h } from "vue";
import { renderToString } from "@vue/server-renderer";
import { WlConfig } from "@whitelife-core/ui-kit";

let server,
  createServerPanel,
  createHttpPanel,
  serverPanelKey,
  operationsFrom,
  apiBaseUrl,
  clientEnvironment;
const capabilities = {
  version: "0.8.30",
  api_version: 2,
  worker_protocol: 4,
  max_audio_seconds: 300,
  max_upload_bytes: 1024,
  devices: [
    { id: "cpu", kind: "cpu" },
    { id: "cuda:0", kind: "cuda", name: "RTX 3060 Ti" },
  ],
  features: { openai_compatible: true, metrics: false },
};
const health = {
  status: "ready",
  models: [{ id: "whisper:tiny", state: "ready", device: "cuda:0" }],
  queue: { interactive: 2, batch: 3 },
};
const models = {
  data: [
    {
      id: "whisper:tiny",
      fono: {
        engine: "whisper",
        languages: ["en", "ru"],
        size_bytes: 1000,
        state: "ready",
        disk_state: "present_unverified",
        device: "cuda:0",
        default: true,
      },
    },
  ],
};
const spec = {
  paths: {
    "/v1/health": { get: { summary: "Health" } },
    "/v1/recognize": { post: { summary: "Recognize" } },
    "/v1/models/{id}/unload": { post: { summary: "Unload" } },
  },
};
function response(status, data, retryAfter = 1) {
  return {
    status,
    data,
    text: JSON.stringify(data),
    requestId: "test-request",
    elapsedMs: 12,
    retryAfter,
  };
}
function fixture() {
  let now = 100000,
    handler;
  const calls = [],
    clientCalls = [],
    clipboard = [],
    timers = new Set();
  let clientHandler;
  const data = {
    "/v1/capabilities": capabilities,
    "/v1/health": health,
    "/v1/models": models,
    "/openapi.json": spec,
  };
  const port = {
    copyText: async (value) => {
      clipboard.push(value);
    },
    requestClients: async (request, token, signal) => {
      clientCalls.push({ ...request, token, signal });
      return clientHandler
        ? clientHandler(request, token, signal)
        : response(200, { data: [], api_base_url: "http://fono-server:17832" });
    },
    connection: "HTTPS",
    now: () => now,
    schedule(callback, delay) {
      const entry = { callback, delay };
      timers.add(entry);
      return () => timers.delete(entry);
    },
    chooseAudio: async () => ({
      name: "sample.wav",
      size: 4,
      source: new Blob(["test"]),
    }),
    request: async (request, token, signal) => {
      calls.push({ ...request, token, signal });
      if (handler) return handler(request, token, signal);
      return response(
        200,
        structuredClone(data[request.path] ?? { text: "Recognized" }),
      );
    },
  };
  return {
    port,
    calls,
    clientCalls,
    clipboard,
    timers,
    setHandler(value) {
      handler = value;
    },
    setClientHandler(value) {
      clientHandler = value;
    },
    advance(ms) {
      now += ms;
    },
  };
}
before(async () => {
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { hmr: false, middlewareMode: true },
  });
  ({ createServerPanel, serverPanelKey } = await server.ssrLoadModule(
    "/src/v3/features/serverPanel/application/panel.ts",
  ));
  ({ createHttpPanel } = await server.ssrLoadModule(
    "/src/v3/features/serverPanel/infrastructure/httpPanel.ts",
  ));
  ({ operationsFrom, apiBaseUrl, clientEnvironment } =
    await server.ssrLoadModule("/src/v3/features/serverPanel/domain/panel.ts"));
});
after(async () => server?.close());

test("login reads real API data and logout clears credentials, data, audio and timers", async () => {
  const f = fixture(),
    panel = createServerPanel(f.port);
  await panel.login("private-token");
  assert.equal(panel.state.authenticated, true);
  assert.equal(panel.state.capabilities.version, "0.8.30");
  assert.equal(panel.state.health.queue.batch, 3);
  assert.equal(panel.state.models[0].fono.device, "cuda:0");
  assert.equal(panel.state.operations.length, 3);
  assert.equal(JSON.stringify(panel.state).includes("private-token"), false);
  assert.deepEqual(
    f.calls.map((c) => c.path),
    ["/v1/capabilities", "/v1/health", "/v1/models", "/openapi.json"],
  );
  await panel.chooseAudio();
  panel.logout();
  assert.equal(f.timers.size, 0);
  assert.equal(panel.state.authenticated, false);
  assert.equal(panel.state.capabilities, null);
  assert.equal(panel.state.health, null);
  assert.equal(panel.state.audio, null);
  assert.deepEqual(panel.state.models, []);
  assert.equal(panel.state.specification, "");
  const count = f.calls.length;
  await panel.refresh();
  await panel.run({ method: "POST", path: "/v1/recognize" });
  assert.equal(f.calls.length, count);
});

test("model startup 503 is a valid authenticated status", async () => {
  const f = fixture(),
    panel = createServerPanel(f.port);
  f.setHandler((request) =>
    response(
      request.path === "/v1/health" ? 503 : 200,
      request.path === "/v1/capabilities"
        ? capabilities
        : request.path === "/v1/health"
          ? { ...health, status: "starting" }
          : request.path === "/v1/models"
            ? models
            : spec,
    ),
  );
  await panel.login("valid-token");
  assert.equal(panel.state.authenticated, true);
  assert.equal(panel.state.health.status, "starting");
  panel.dispose();
});

test("invalid and revoked tokens never retain an authenticated snapshot", async () => {
  const f = fixture(),
    panel = createServerPanel(f.port);
  f.setHandler(() => response(401, { error: { code: "unauthorized" } }));
  await panel.login("bad-token");
  assert.equal(panel.state.authenticated, false);
  assert.match(panel.state.error, /недействителен/);
  f.setHandler(null);
  await panel.login("valid-token");
  f.setHandler(() => response(401, {}));
  await panel.refresh(true);
  assert.equal(panel.state.authenticated, false);
  assert.equal(panel.state.health, null);
  assert.equal(f.timers.size, 0);
});

test("rate limiting honors Retry-After without a polling loop or repeated audio request", async () => {
  const f = fixture(),
    panel = createServerPanel(f.port);
  await panel.login("valid-token");
  f.setHandler(() => response(429, { error: { code: "rate_limited" } }, 60));
  await panel.refresh(true);
  assert.equal(panel.state.retryAt, 160000);
  assert.equal([...f.timers][0].delay, 60000);
  const count = f.calls.length;
  await panel.refresh(true);
  await panel.run({ method: "POST", path: "/v1/recognize" });
  assert.equal(f.calls.length, count);
  f.advance(60000);
  f.setHandler(null);
  await panel.refresh();
  assert.equal(panel.state.error, "");
  assert.equal(panel.state.retryAt, 0);
  assert.equal(f.calls.filter((c) => c.method === "POST").length, 0);
  panel.dispose();
});

test("logout aborts an in-flight refresh and discards a late previous-session result", async () => {
  const f = fixture(),
    panel = createServerPanel(f.port);
  await panel.login("valid-token");
  let finish;
  f.setHandler(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const pending = panel.refresh(true);
  const signal = f.calls.at(-1).signal;
  panel.logout();
  assert.equal(signal.aborted, true);
  finish(response(200, health));
  await pending;
  assert.equal(panel.state.health, null);
  assert.equal(f.timers.size, 0);
});

test("cancelled audio request cannot overwrite a subsequent request", async () => {
  const f = fixture(),
    panel = createServerPanel(f.port);
  await panel.login("valid-token");
  let finish;
  f.setHandler(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const first = panel.run({ method: "POST", path: "/v1/recognize" });
  const signal = f.calls.at(-1).signal;
  panel.cancelRequest();
  assert.equal(signal.aborted, true);
  f.setHandler(() => response(200, { text: "new-result" }));
  await panel.run({ method: "GET", path: "/v1/health" });
  finish(response(200, { text: "old-result" }));
  await first;
  assert.equal(panel.state.response.data.text, "new-result");
  assert.equal(panel.state.requestBusy, false);
  panel.dispose();
});

test("audio size and explicit selection are validated before upload", async () => {
  const f = fixture(),
    panel = createServerPanel(f.port);
  await panel.login("valid-token");
  f.port.chooseAudio = async () => ({
    name: "large.wav",
    size: 1025,
    source: new Blob(),
  });
  await panel.chooseAudio();
  assert.equal(panel.state.audio, null);
  assert.match(panel.state.requestError, /превышает/);
  assert.equal(f.calls.filter((c) => c.method === "POST").length, 0);
  panel.dispose();
});

test("network errors preserve the last snapshot and disabling polling removes timers", async () => {
  const f = fixture(),
    panel = createServerPanel(f.port);
  await panel.login("valid-token");
  f.setHandler(() => {
    throw new Error("secret-native-details");
  });
  await panel.refresh();
  assert.equal(panel.state.health.status, "ready");
  assert.match(panel.state.error, /последние/);
  assert.equal(panel.state.error.includes("secret-native-details"), false);
  panel.setAutoRefresh(false);
  assert.equal(f.timers.size, 0);
  panel.dispose();
});

test("HTTP adapter keeps bearer authentication same-origin, uncached, and redirect-free", async () => {
  const calls = [];
  const port = createHttpPanel(async (path, options) => {
    calls.push({ path, options });
    return new Response('{"status":"ready"}', {
      headers: { "X-Request-Id": "safe-id" },
    });
  });
  const result = await port.request(
    { method: "GET", path: "/v1/health" },
    "private-token",
    new AbortController().signal,
  );
  assert.equal(result.data.status, "ready");
  assert.equal(result.requestId, "safe-id");
  const { path, options } = calls[0];
  assert.equal(path, "/v1/health");
  assert.equal(options.headers.get("Authorization"), "Bearer private-token");
  assert.equal(options.credentials, "omit");
  assert.equal(options.mode, "same-origin");
  assert.equal(options.redirect, "error");
  assert.equal(options.cache, "no-store");
  for (const path of [
    "https://evil.example/",
    "//evil.example/",
    "/metrics",
    "/v1/models/x/unload",
  ]) {
    await assert.rejects(
      port.request(
        { method: "GET", path },
        "private-token",
        new AbortController().signal,
      ),
    );
  }
  assert.equal(calls.length, 1);
});

test("playground emits WAV body or OpenAI multipart and never exposes the token in the URL", async () => {
  const calls = [],
    signal = new AbortController().signal;
  const port = createHttpPanel(async (path, options) => {
    calls.push({ path, options });
    return new Response("recognized text", {
      headers: { "Content-Type": "text/plain" },
    });
  });
  const audio = { name: "sample.wav", size: 4, source: new Blob(["wave"]) };
  await port.request(
    {
      method: "POST",
      path: "/v1/recognize",
      model: "whisper:tiny",
      language: "ru",
      timestamps: "segments",
    },
    "private-token",
    signal,
    audio,
  );
  assert.equal(calls[0].options.body, audio.source);
  assert.equal(calls[0].options.headers.get("Content-Type"), "audio/wav");
  assert.match(calls[0].path, /language=ru/);
  assert.match(calls[0].path, /timestamps=segments/);
  const result = await port.request(
    {
      method: "POST",
      path: "/v1/audio/transcriptions",
      model: "whisper:tiny",
      language: "auto",
      format: "text",
    },
    "private-token",
    signal,
    audio,
  );
  assert.equal(result.text, "recognized text");
  assert.equal(calls[1].options.headers.has("Content-Type"), false);
  assert.equal(calls[1].options.body.get("model"), "whisper:tiny");
  assert.equal(calls[1].options.body.has("language"), false);
  assert.equal(calls[1].options.body.get("response_format"), "text");
  assert.equal(calls[1].options.body.get("file").name, "sample.wav");
  assert.ok(calls.every((c) => !c.path.includes("private-token")));
});

test("OpenAPI renders all operations while mutation routes remain documentation-only", () => {
  const operations = operationsFrom(spec);
  assert.equal(operations.length, 3);
  assert.equal(
    operations.find((o) => o.path.includes("unload")).runnable,
    false,
  );
  assert.equal(
    operations.find((o) => o.path === "/v1/recognize").runnable,
    true,
  );
});

test("a rate-limited audio probe keeps the actual HTTP error visible without retrying", async () => {
  const f = fixture(),
    panel = createServerPanel(f.port);
  await panel.login("valid-token");
  f.setHandler(() => response(429, { error: { code: "rate_limited" } }, 15));
  await panel.run({ method: "POST", path: "/v1/recognize" });
  assert.equal(panel.state.response.status, 429);
  assert.equal(panel.state.response.data.error.code, "rate_limited");
  assert.equal(panel.state.retryAt, 115000);
  assert.equal(f.calls.filter((c) => c.method === "POST").length, 1);
  panel.dispose();
});

test("malformed tokens are rejected before any network request", async () => {
  for (const value of [
    "",
    "token with spaces",
    "token\r\nInjected: value",
    "x".repeat(4097),
  ]) {
    const f = fixture(),
      panel = createServerPanel(f.port);
    await panel.login(value);
    assert.equal(f.calls.length, 0);
    assert.equal(panel.state.authenticated, false);
    panel.dispose();
  }
});

test("the HTTP adapter forwards cancellation while a response is pending", async () => {
  const caller = new AbortController();
  let observed;
  const port = createHttpPanel(
    async (_path, options) =>
      new Promise((_resolve, reject) => {
        observed = options.signal;
        options.signal.addEventListener(
          "abort",
          () => reject(new Error("aborted")),
          { once: true },
        );
      }),
  );
  const pending = port.request(
    { method: "GET", path: "/v1/health" },
    "valid-token",
    caller.signal,
  );
  caller.abort();
  await assert.rejects(pending);
  assert.equal(observed.aborted, true);
});

test("Vue panel renders login and real snapshots with escaped server text", async () => {
  const { default: component } = await server.ssrLoadModule(
    "/src/v3/features/serverPanel/presentation/ServerPanel.vue",
  );
  const f = fixture(),
    panel = createServerPanel(f.port);
  const render = async () => {
    const app = createSSRApp({ render: () => h(component) });
    app.use(WlConfig);
    app.provide(serverPanelKey, panel);
    return renderToString(app);
  };
  assert.match(await render(), /Токен доступа/);
  await panel.login("private-token");
  const status = await render();
  assert.match(status, /RTX 3060 Ti/);
  assert.equal(status.includes("private-token"), false);
  panel.state.tab = "models";
  panel.state.models[0].id = '<img src=x onerror="alert(1)">';
  const table = await render();
  assert.match(table, /&lt;img/);
  assert.equal(table.includes("<img src=x"), false);
  panel.state.tab = "queue";
  assert.match(await render(), /Интерактивные запросы/);
  panel.state.tab = "docs";
  assert.match(await render(), /Пробный запрос/);
  panel.state.tab = "clients";
  await new Promise(setImmediate);
  const clients = await render();
  assert.equal((clients.match(/Подключить приложение/g) ?? []).length, 1);
  assert.match(clients, /Адрес API для приложения/);
  assert.match(clients, /В Docker localhost/);
  panel.dispose();
});

const issuedToken = `fono_${"a".repeat(64)}`;
function created(name = "my-app") {
  return response(201, {
    name,
    scope: "recognize",
    created_at: "2026-11-06T12:00:00Z",
    token: issuedToken,
  });
}
async function clientFixture() {
  const f = fixture(),
    panel = createServerPanel(f.port);
  await panel.login("admin-token");
  assert.equal(
    f.clientCalls.length,
    0,
    "management is not probed during ordinary login",
  );
  panel.state.tab = "clients";
  await new Promise(setImmediate);
  return { f, panel };
}
test("client onboarding creates one key, copies env, and clears the secret when hidden", async () => {
  const { f, panel } = await clientFixture();
  assert.equal(panel.state.clientUrl, "http://fono-server:17832");
  f.setClientHandler((request) => created(request.name));
  panel.state.clientName = "my-app";
  await panel.createClient();
  assert.equal(
    panel.state.clientEnv,
    `FONO_HOST=http://fono-server:17832\nFONO_TOKEN=${issuedToken}`,
  );
  assert.equal(panel.state.clients[0].scope, "recognize");
  assert.equal(
    JSON.stringify(panel.state.clients).includes(issuedToken),
    false,
  );
  assert.equal(JSON.stringify(panel.state).includes("admin-token"), false);
  await panel.copyClientEnvironment();
  assert.equal(f.clipboard[0], panel.state.clientEnv);
  await panel.createClient();
  assert.equal(
    f.clientCalls.filter((request) => request.method === "POST").length,
    1,
  );
  panel.state.tab = "status";
  assert.equal(panel.state.clientEnv, "");
  assert.equal(panel.state.issuedClient, "");
  panel.dispose();
});
test("client address is never inferred from the browser tunnel and dotenv injection is rejected", async () => {
  assert.equal(
    apiBaseUrl("https://fono.internal:17832/"),
    "https://fono.internal:17832",
  );
  assert.equal(apiBaseUrl("http://[::1]:17832"), "http://[::1]:17832");
  for (const url of [
    "",
    "http://host\nINJECT=1",
    "http://user:pass@host",
    "https://host/api",
    "https://host?",
    "https://host#",
    "http://0.0.0.0:17832",
    "http://[::]:17832",
    "http://host:0",
    "http://host/$(cmd)",
    "file:///host",
    "//host",
    "https://host\\evil",
    "http://host;command",
    "http://host&command",
    "http://host|command",
  ]) {
    assert.equal(apiBaseUrl(url), null, url);
    assert.equal(clientEnvironment(url, issuedToken), null, url);
  }
  assert.equal(clientEnvironment("http://host", "token\nEVIL=1"), null);
  const { f, panel } = await clientFixture();
  f.setClientHandler(() => response(200, { data: [], api_base_url: null }));
  panel.state.clientUrl = "";
  await panel.loadClients();
  assert.equal(panel.state.clientUrl, "");
  panel.state.clientName = "my-app";
  await panel.createClient();
  assert.match(panel.state.clientError, /Укажите адрес/);
  assert.equal(
    f.clientCalls.filter((request) => request.method === "POST").length,
    0,
  );
  panel.dispose();
});
test("ordinary recognition credentials see an admin explanation and cannot create keys", async () => {
  const { f, panel } = await clientFixture();
  f.setClientHandler(() => response(403, { error: { code: "forbidden" } }));
  await panel.loadClients();
  assert.equal(panel.state.clientsAllowed, false);
  assert.match(panel.state.clientError, /admin-токеном/);
  panel.state.clientName = "my-app";
  await panel.createClient();
  assert.equal(
    f.clientCalls.filter((request) => request.method === "POST").length,
    0,
  );
  assert.equal(panel.state.authenticated, true);
  panel.dispose();
});
test("late created secrets are discarded after switching tabs, logout, or a new session", async () => {
  for (const leave of [
    (panel) => {
      panel.state.tab = "status";
    },
    (panel) => panel.logout(),
  ]) {
    const { f, panel } = await clientFixture();
    let finish;
    f.setClientHandler(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    panel.state.clientName = "my-app";
    const pending = panel.createClient();
    const signal = f.clientCalls.at(-1).signal;
    await panel.createClient();
    assert.equal(
      f.clientCalls.filter((request) => request.method === "POST").length,
      1,
    );
    leave(panel);
    assert.equal(signal.aborted, true);
    finish(created());
    await pending;
    assert.equal(panel.state.clientEnv, "");
    assert.equal(panel.state.issuedClient, "");
    panel.dispose();
  }
});
test("revocation requires a selected confirmation and clears a newly issued secret", async () => {
  const { f, panel } = await clientFixture();
  f.setClientHandler((request) =>
    request.method === "DELETE" ? response(204, null) : created(request.name),
  );
  panel.state.clientName = "my-app";
  await panel.createClient();
  await panel.revokeClient();
  assert.equal(
    f.clientCalls.filter((request) => request.method === "DELETE").length,
    0,
  );
  panel.askRevoke("unknown-admin");
  assert.equal(panel.state.revokeCandidate, "");
  panel.askRevoke("my-app");
  panel.cancelRevoke();
  await panel.revokeClient();
  assert.equal(
    f.clientCalls.filter((request) => request.method === "DELETE").length,
    0,
  );
  panel.askRevoke("my-app");
  await panel.revokeClient();
  assert.equal(panel.state.clients.length, 0);
  assert.equal(panel.state.clientEnv, "");
  assert.equal(
    f.clientCalls.filter((request) => request.method === "DELETE").length,
    1,
  );
  panel.dispose();
});
test("clipboard failures preserve selectable credentials without leaking native error text", async () => {
  const { f, panel } = await clientFixture();
  f.setClientHandler(() => created());
  f.port.copyText = async () => {
    throw new Error("private-browser-detail");
  };
  panel.state.clientName = "my-app";
  await panel.createClient();
  await panel.copyClientEnvironment();
  assert.match(panel.state.clientNotice, /Выделите и скопируйте/);
  assert.equal(
    panel.state.clientNotice.includes("private-browser-detail"),
    false,
  );
  assert.ok(panel.state.clientEnv.includes(issuedToken));
  panel.dispose();
  assert.equal(panel.state.clientEnv, "");
});
test("revoked sessions and quotas prevent key mutations without replay", async () => {
  const { f, panel } = await clientFixture();
  panel.state.clientName = "my-app";
  f.setClientHandler(() => response(429, {}, 60));
  await panel.createClient();
  assert.equal(panel.state.retryAt, 160000);
  const count = f.clientCalls.length;
  await panel.createClient();
  assert.equal(f.clientCalls.length, count);
  f.advance(60000);
  f.setClientHandler(() => response(401, {}));
  await panel.createClient();
  assert.equal(panel.state.authenticated, false);
  assert.equal(panel.state.clientEnv, "");
  panel.dispose();
});
test("unknown mutation outcome is not automatically retried and raw errors stay private", async () => {
  const { f, panel } = await clientFixture();
  f.setClientHandler(() => {
    throw new Error("private-store-path");
  });
  panel.state.clientName = "my-app";
  await panel.createClient();
  assert.match(panel.state.clientError, /могла выполниться/);
  assert.equal(panel.state.clientError.includes("private-store-path"), false);
  assert.equal(
    f.clientCalls.filter((request) => request.method === "POST").length,
    1,
  );
  panel.dispose();
});
test("client HTTP requests use fixed routes, bounded names and admin JSON without changing playground", async () => {
  const calls = [],
    signal = new AbortController().signal;
  const port = createHttpPanel(async (path, options) => {
    calls.push({ path, options });
    return new Response(null, { status: 204 });
  });
  await port.requestClients({ method: "GET" }, "admin-token", signal);
  await port.requestClients(
    { method: "POST", name: "my-app" },
    "admin-token",
    signal,
  );
  await port.requestClients(
    { method: "DELETE", name: "my-app" },
    "admin-token",
    signal,
  );
  assert.deepEqual(
    calls.map((call) => call.path),
    ["/v1/clients", "/v1/clients", "/v1/clients/my-app"],
  );
  assert.equal(calls[1].options.body, '{"name":"my-app"}');
  assert.equal(
    calls[1].options.headers.get("Content-Type"),
    "application/json",
  );
  for (const { options } of calls) {
    assert.equal(options.headers.get("Authorization"), "Bearer admin-token");
    assert.equal(options.redirect, "error");
    assert.equal(options.credentials, "omit");
    assert.equal(options.mode, "same-origin");
    assert.equal(options.cache, "no-store");
  }
  for (const request of [
    { method: "GET", name: "x" },
    { method: "POST", name: "../admin" },
    { method: "POST", name: "x", scope: "admin" },
    { method: "POST", name: "x\r\n" },
    { method: "POST", name: "x".repeat(65) },
    { method: "DELETE", name: "x?admin=1" },
    { method: "PUT", name: "x" },
    { method: "GET", path: "https://evil.test" },
  ]) {
    await assert.rejects(port.requestClients(request, "admin-token", signal));
  }
  await assert.rejects(
    port.request(
      { method: "POST", path: "/v1/clients" },
      "admin-token",
      signal,
    ),
  );
  assert.equal(calls.length, 3);
});
