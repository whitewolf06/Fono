import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer } from "vite";
import { createSSRApp, h } from "vue";
import { renderToString } from "vue/server-renderer";
import { WlConfig } from "@whitelife-core/ui-kit";

let server,
  createWakeSamples,
  createNativeWakeSamples,
  createMockWakeSamples,
  workspaceKey,
  Panel;
const request = {
  language: "ru",
  kind: "phrase",
  condition: "quiet",
  consent: true,
};
const sample = {
  id: "sample-one",
  language: "ru",
  kind: "phrase",
  condition: "quiet",
  durationMs: 5000,
  sha256: "a".repeat(64),
  recordedAtMs: 1,
};
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((a, b) => {
    resolve = a;
    reject = b;
  });
  return { promise, resolve, reject };
};
function backend(patch = {}) {
  return {
    list: async () => [],
    start: async () => ({ sessionId: 1, durationMs: 5000 }),
    finish: async () => sample,
    cancel: async () => {},
    audio: async () => ({ url: "blob:test-audio", release() {} }),
    remove: async () => {},
    ...patch,
  };
}
before(async () => {
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { hmr: false, middlewareMode: true },
  });
  ({ createWakeSamples } = await server.ssrLoadModule(
    "/src/v3/features/wake-samples/application/recording.ts",
  ));
  ({ createNativeWakeSamples } = await server.ssrLoadModule(
    "/src/v3/features/wake-samples/infrastructure/nativeSamples.ts",
  ));
  ({ createMockWakeSamples } = await server.ssrLoadModule(
    "/src/v3/features/wake-samples/infrastructure/mockSamples.ts",
  ));
  ({ workspaceKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/workspace.ts",
  ));
  ({ default: Panel } = await server.ssrLoadModule(
    "/src/v3/features/wake-samples/presentation/WakeSamplesPanel.vue",
  ));
});
after(async () => await server?.close());

test("recording requires explicit local voice consent before any backend start", async () => {
  let starts = 0;
  const port = createWakeSamples(
    backend({
      start: async () => {
        starts++;
        throw new Error("unexpected");
      },
    }),
  );
  await port.record({ ...request, consent: false });
  assert.equal(starts, 0);
  assert.match(port.state.error, /Подтвердите/);
  assert.equal(port.state.phase, "idle");
  port.dispose();
});
test("a cancelled late start is cancelled by its own identity and cannot start a timer", async () => {
  const start = deferred();
  const cancelled = [];
  const port = createWakeSamples(
    backend({
      start: () => start.promise,
      cancel: async (id) => cancelled.push(id),
    }),
  );
  const pending = port.record(request);
  assert.equal(port.state.phase, "starting");
  const cancel = port.cancel();
  start.resolve({ sessionId: 42, durationMs: 5000 });
  await Promise.all([pending, cancel]);
  assert.deepEqual(cancelled, [42]);
  assert.equal(port.state.phase, "idle");
  assert.equal(port.state.remainingMs, 0);
  port.dispose();
});
test("repeated save awaits one result, with no dictation history or insertion call", async () => {
  const save = deferred();
  let finishes = 0;
  const port = createWakeSamples(
    backend({
      finish: async (id) => {
        assert.equal(id, 1);
        finishes++;
        return save.promise;
      },
    }),
  );
  await port.record(request);
  const first = port.finish();
  const second = port.finish();
  assert.equal(port.state.phase, "saving");
  assert.equal(finishes, 1);
  save.resolve(sample);
  await Promise.all([first, second]);
  assert.equal(port.state.samples.length, 1);
  assert.equal(port.state.phase, "idle");
  port.dispose();
});
test("cancel during save ignores a late result and waits before allowing a successor", async () => {
  const save = deferred();
  const port = createWakeSamples(backend({ finish: () => save.promise }));
  await port.record(request);
  const finish = port.finish();
  const cancel = port.cancel();
  assert.equal(port.state.phase, "cancelling");
  await port.record(request);
  assert.equal(port.state.phase, "cancelling");
  save.resolve(sample);
  await Promise.all([finish, cancel]);
  assert.equal(port.state.samples.length, 0);
  assert.equal(port.state.phase, "idle");
  await port.record(request);
  assert.equal(port.state.phase, "recording");
  await port.cancel();
  port.dispose();
});
test("late list and audio responses cannot resurrect a deleted sample", async () => {
  const list = deferred(),
    audio = deferred();
  const port = createWakeSamples(
    backend({ list: () => list.promise, audio: () => audio.promise }),
  );
  port.state.samples = [sample];
  const pendingList = port.load();
  const pendingAudio = port.listen(sample.id);
  await port.remove(sample.id);
  let released = false;
  list.resolve([sample]);
  audio.resolve({
    url: "blob:test-audio",
    release: () => {
      released = true;
    },
  });
  await Promise.all([pendingList, pendingAudio]);
  assert.equal(port.state.samples.length, 0);
  assert.equal(port.state.audio, null);
  assert.equal(released, true);
  port.dispose();
});
test("a failed delete preserves the recording and reports the failure", async () => {
  const port = createWakeSamples(
    backend({
      remove: async () => {
        throw new Error("delete failed");
      },
    }),
  );
  port.state.samples = [sample];
  await port.remove(sample.id);
  assert.equal(port.state.samples.length, 1);
  assert.equal(port.state.error, "delete failed");
  port.dispose();
});
test("playback resources are revoked when replacing or leaving the form", async () => {
  let released = 0;
  const port = createWakeSamples(
    backend({
      audio: async () => ({ url: "blob:local", release: () => released++ }),
    }),
  );
  port.state.samples = [sample];
  await port.listen(sample.id);
  assert.equal(port.state.audio.url, "blob:local");
  await port.listen(sample.id);
  assert.equal(released, 1);
  await port.cancel();
  assert.equal(released, 2);
  assert.equal(port.state.audio, null);
  await port.listen(sample.id);
  port.dispose();
  assert.equal(released, 3);
});
test("a bounded recording automatically saves once and releases its timer", async () => {
  let finished = 0;
  const port = createWakeSamples(
    backend({
      start: async () => ({ sessionId: 5, durationMs: 1 }),
      finish: async (id) => {
        assert.equal(id, 5);
        finished++;
        return sample;
      },
    }),
  );
  await port.record(request);
  await new Promise((resolve) => setTimeout(resolve, 220));
  assert.equal(finished, 1);
  assert.equal(port.state.phase, "idle");
  assert.equal(port.state.remainingMs, 0);
  port.dispose();
});
test("browser preview never creates microphone data or a successful fake sample", async () => {
  const port = createMockWakeSamples();
  await port.load();
  await port.record(request);
  assert.match(port.state.error, /приложении Fono/);
  assert.equal(port.state.phase, "idle");
  assert.deepEqual([...port.state.samples], []);
  port.dispose();
});
test("native adapter uses only the new raw-sample commands and cancels on hide", async () => {
  const calls = [],
    events = new Map();
  globalThis.document = {
    visibilityState: "visible",
    addEventListener: (name, action) => events.set(name, action),
    removeEventListener: (name) => events.delete(name),
  };
  const port = createNativeWakeSamples(async (name, args) => {
    calls.push([name, args]);
    if (name === "list_wake_samples") return [];
    if (name === "start_wake_sample")
      return { sessionId: 81, durationMs: 5000 };
  });
  try {
    await port.load();
    await port.record(request);
    globalThis.document.visibilityState = "hidden";
    events.get("visibilitychange")();
    await new Promise((resolve) => setImmediate(resolve));
    assert.deepEqual(
      calls.map((c) => c[0]),
      ["list_wake_samples", "start_wake_sample", "cancel_wake_sample"],
    );
    assert.deepEqual(calls[2][1], { sessionId: 81 });
    assert.equal(port.state.phase, "idle");
  } finally {
    port.dispose();
    delete globalThis.document;
  }
  assert.equal(events.size, 0);
});
test("recording form explains preparation and local consent, without a wake activation switch", async () => {
  const port = createMockWakeSamples();
  const app = createSSRApp({ render: () => h(Panel) });
  app.use(WlConfig);
  app.provide(workspaceKey, {
    native: false,
    wakeSamples: port,
    state: { phase: "idle" },
  });
  const html = await renderToString(app);
  assert.match(html, /Записи для WakeWord/);
  assert.match(html, /пробуждение пока выключено/);
  assert.match(html, /не отправляет их на сервер/);
  assert.doesNotMatch(html, /wakeEnabled/);
  assert.match(html, /<button[^>]*disabled/);
  port.dispose();
});
