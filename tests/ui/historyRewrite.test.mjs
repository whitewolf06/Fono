import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";

let server, editor, mock, native;
before(async () => {
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { hmr: false, middlewareMode: true },
  });
  editor = await server.ssrLoadModule(
    "/src/v3/features/history/application/rewriteController.ts",
  );
  mock = await server.ssrLoadModule(
    "/src/v3/features/history/infrastructure/mockRewrite.ts",
  );
  native = await server.ssrLoadModule(
    "/src/v3/features/history/infrastructure/nativeRewrite.ts",
  );
});
after(async () => server?.close());
const tick = async () => {
  for (let i = 0; i < 10; i++) await Promise.resolve();
};
function deferred() {
  let resolve, reject;
  const promise = new Promise((a, b) => {
    resolve = a;
    reject = b;
  });
  return { promise, resolve, reject };
}
const profile = {
  id: "local",
  name: "Local",
  model: "model",
  revision: "revision",
  cloud: false,
  configured: true,
};
const value = (entryId = "entry", style = "preserve") => ({
  id: `${entryId}-${style}`,
  entryId,
  source: "result",
  sourceFingerprint: `${entryId}-result`,
  style,
  profileId: "local",
  profileName: "Local",
  profileRevision: "revision",
  model: "model",
  createdAt: "2026-10-22T00:00:00Z",
  status: "ready",
  text: "suggested alternative",
  stale: false,
});
function context(entryId = "entry", overrides = {}) {
  return {
    enabled: true,
    profiles: [{ ...profile }],
    defaultProfileId: "local",
    originalFingerprint: `${entryId}-original`,
    resultFingerprint: `${entryId}-result`,
    externalDelivery: "unknown",
    variants: [],
    ...overrides,
  };
}
function setup(overrides = {}) {
  const calls = [];
  const port = {
    createOperationId: () => `job-${calls.length}`,
    context: async (id) => context(id),
    run: async (req) => {
      calls.push(req);
      return value(req.entryId, req.style);
    },
    cancel: async () => {},
    remove: async () => {},
    ...overrides,
  };
  const state = editor.initialRewriteState();
  return {
    calls,
    port,
    state,
    controller: editor.createRewriteController(port, state),
  };
}

test("opening an archive or selecting a style never starts AI; explicit action deduplicates cached results", async () => {
  const { calls, state, controller } = setup();
  await controller.open("entry");
  state.style = "friendly";
  assert.equal(calls.length, 0);
  await controller.generate(() =>
    assert.fail("local profile requires no cloud dialog"),
  );
  assert.equal(calls.length, 1);
  assert.equal(calls[0].style, "friendly");
  assert.equal(calls[0].cloudConsent, false);
  assert.equal(state.context.variants.length, 1);
  await controller.generate(() =>
    assert.fail("cache should not ask for sending consent"),
  );
  assert.equal(calls.length, 1);
  assert.equal(state.selectedId, "entry-friendly");
  state.style = "concise";
  await controller.generate(async () => true);
  assert.equal(calls.length, 2);
  assert.equal(state.context.variants.length, 2);
  controller.dispose();
});

test("no model, disabled history or missing source prevent generation", async () => {
  for (const missing of [
    { profiles: [] },
    { profiles: [{ ...profile, configured: false }] },
    { enabled: false },
    { resultFingerprint: null },
  ]) {
    const { calls, state, controller } = setup({
      context: async () => context("entry", missing),
    });
    await controller.open("entry");
    assert.equal(editor.canRewrite(state), false);
    await controller.generate(async () => true);
    assert.equal(calls.length, 0);
    controller.dispose();
  }
});

test("creating a new alternative does not silently discard earlier saved alternatives", async () => {
  const variants = Array.from({ length: 25 }, (_, index) => ({
    ...value(),
    id: `older-${index}`,
    stale: true,
  }));
  const { state, controller } = setup({
    context: async () => context("entry", { variants }),
  });
  await controller.open("entry");
  await controller.generate(async () => true);
  assert.equal(state.context.variants.length, 26);
  assert.ok(state.context.variants.some((v) => v.id === "older-0"));
  controller.dispose();
});

test("late result from an old record cannot replace the new selection", async () => {
  const pending = deferred(),
    cancelled = [];
  const { state, controller } = setup({
    run: () => pending.promise,
    cancel: async (id) => cancelled.push(id),
  });
  await controller.open("first");
  const old = controller.generate(async () => true);
  await tick();
  await controller.open("second");
  pending.resolve(value("first"));
  await old;
  assert.equal(cancelled.length, 1);
  assert.equal(state.entryId, "second");
  assert.equal(state.context.variants.length, 0);
  assert.equal(state.generating, false);
  controller.dispose();
});

test("cloud consent is bound to the displayed record and cannot survive a selection change", async () => {
  const consent = deferred();
  const { calls, state, controller } = setup({
    context: async (id) =>
      context(id, { profiles: [{ ...profile, cloud: true }] }),
  });
  await controller.open("first");
  const old = controller.generate(() => consent.promise);
  await tick();
  await controller.open("second");
  consent.resolve(true);
  await old;
  assert.equal(calls.length, 0);
  assert.equal(state.entryId, "second");
  await controller.generate(async () => false);
  assert.equal(calls.length, 0);
  controller.dispose();
});

test("explicit cloud consent sends only the selected source and profile revision", async () => {
  const { calls, state, controller } = setup({
    context: async (id) =>
      context(id, { profiles: [{ ...profile, cloud: true }] }),
  });
  await controller.open("entry");
  state.source = "original";
  await controller.generate(async (p) => {
    assert.equal(p.id, "local");
    return true;
  });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].cloudConsent, true);
  assert.equal(calls[0].sourceFingerprint, "entry-original");
  assert.equal(calls[0].profileRevision, "revision");
  assert.ok(!("baseUrl" in calls[0]));
  assert.ok(!("text" in calls[0]));
  controller.dispose();
});

test("cancel and provider errors keep source and previous variants, never a false ready result", async () => {
  const pending = deferred();
  const { state, controller } = setup({
    run: () => pending.promise,
    cancel: async () => pending.reject(new Error("PRIVATE_TEXT")),
  });
  await controller.open("entry");
  const run = controller.generate(async () => true);
  await tick();
  await controller.cancel();
  await run;
  assert.equal(state.generating, false);
  assert.equal(state.error, "");
  assert.equal(state.context.variants.length, 0);
  controller.dispose();
  const failed = setup({
    run: async () => {
      throw new Error("PRIVATE_TEXT");
    },
  });
  await failed.controller.open("entry");
  await failed.controller.generate(async () => true);
  assert.ok(failed.state.error);
  assert.ok(!failed.state.error.includes("PRIVATE_TEXT"));
  assert.equal(failed.state.context.variants.length, 0);
  failed.controller.dispose();
});

test("saved stale variants are viewable but not reused; deletion reloads the archive cache", async () => {
  let variants = [{ ...value(), stale: true }];
  let removed = 0;
  const { calls, state, controller } = setup({
    context: async () => context("entry", { variants }),
    remove: async (entry, id) => {
      assert.equal(entry, "entry");
      assert.equal(id, "entry-preserve");
      variants = [];
      removed++;
    },
  });
  await controller.open("entry");
  assert.equal(editor.selectedRewrite(state).stale, true);
  assert.equal(editor.matchesRewrite(state, variants[0]), false);
  await controller.remove();
  assert.equal(removed, 1);
  assert.equal(state.context.variants.length, 0);
  await controller.generate(async () => true);
  assert.equal(calls.length, 1);
  controller.dispose();
});

test("native archive editor uses typed isolated commands without renderer-supplied text, endpoint or model", async () => {
  const calls = [];
  const port = native.createNativeHistoryRewrite(async (command, args) => {
    calls.push({ command, args });
    return command === "get_history_rewrite_context" ? context() : value();
  });
  const operationId = port.createOperationId();
  assert.ok(operationId.length > 10);
  await port.context("entry");
  await port.run({
    operationId,
    entryId: "entry",
    source: "result",
    sourceFingerprint: "source",
    style: "preserve",
    profileId: "local",
    profileRevision: "revision",
    cloudConsent: false,
  });
  await port.cancel(operationId);
  await port.remove("entry", "variant");
  assert.deepEqual(
    calls.map((c) => c.command),
    [
      "get_history_rewrite_context",
      "create_history_rewrite",
      "cancel_history_rewrite",
      "delete_history_rewrite",
    ],
  );
  assert.equal(calls[1].args.request.operationId, operationId);
  assert.equal(calls[3].args.variantId, "variant");
});

function demoState() {
  return {
    preferences: {
      historyEnabled: true,
      analyticsConsent: true,
      profile: "local",
      processingModel: "model",
    },
    profiles: [
      {
        id: "local",
        name: "Local",
        provider: "lmstudio",
        location: "local",
        url: "http://localhost:1234/v1",
        model: "model",
      },
    ],
    history: [
      {
        id: "entry",
        createdAt: "2026-10-22T00:00:00Z",
        text: "Final demo text",
        original: "Original demo text",
      },
    ],
  };
}

test("a delayed delete confirmation cannot remove an alternative from another record", async () => {
  let removed = 0;
  const { state, controller } = setup({
    context: async (id) => context(id, { variants: [value(id)] }),
    remove: async () => {
      removed++;
    },
  });
  await controller.open("first");
  const entry = state.entryId,
    variant = state.selectedId;
  await controller.open("second");
  await controller.remove(entry, variant);
  assert.equal(removed, 0);
  assert.equal(state.selectedId, "second-preserve");
  controller.dispose();
});
async function demoRequest(port, source = "result") {
  const c = await port.context("entry");
  return {
    operationId: port.createOperationId(),
    entryId: "entry",
    source,
    sourceFingerprint:
      source === "original" ? c.originalFingerprint : c.resultFingerprint,
    style: "preserve",
    profileId: c.profiles[0].id,
    profileRevision: c.profiles[0].revision,
    cloudConsent: false,
  };
}
test("browser mock keeps variants in memory, reopens cache, prunes deleted sources and never changes transcript", async () => {
  const state = demoState(),
    before = JSON.stringify(state.history),
    runtime = mock.createMockHistoryRewrite(state);
  const request = await demoRequest(runtime.port);
  const first = await runtime.port.run(request);
  assert.equal(first.text, state.history[0].text);
  assert.equal(JSON.stringify(state.history), before);
  assert.equal((await runtime.port.context("entry")).variants.length, 1);
  assert.equal(
    (
      await runtime.port.run({
        ...request,
        operationId: runtime.port.createOperationId(),
      })
    ).createdAt,
    first.createdAt,
  );
  await runtime.port.run(await demoRequest(runtime.port, "original"));
  state.preferences.analyticsConsent = false;
  delete state.history[0].original;
  runtime.prune();
  assert.equal((await runtime.port.context("entry")).variants.length, 1);
  state.history = [];
  runtime.prune();
  state.history = demoState().history;
  assert.equal((await runtime.port.context("entry")).variants.length, 0);
  assert.equal(
    (await mock.createMockHistoryRewrite(state).port.context("entry")).variants
      .length,
    0,
  );
  runtime.dispose();
});

test("browser mock cancels pending work and does not recreate a deleted record", async () => {
  for (const action of ["cancel", "delete"]) {
    const state = demoState(),
      runtime = mock.createMockHistoryRewrite(state);
    const request = await demoRequest(runtime.port),
      pending = runtime.port.run(request);
    await new Promise((resolve) => setTimeout(resolve, 20));
    if (action === "cancel") await runtime.port.cancel(request.operationId);
    else {
      state.history = [];
      runtime.prune();
    }
    await assert.rejects(pending);
    state.history = demoState().history;
    assert.equal((await runtime.port.context("entry")).variants.length, 0);
    runtime.dispose();
  }
});
