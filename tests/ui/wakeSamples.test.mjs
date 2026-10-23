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
  wakeSamplesInSet,
  Panel;
const request = {
  phraseId: "privet_lira",
  language: "ru",
  kind: "phrase",
  condition: "quiet",
  consent: true,
};
const sample = {
  id: "sample-one",
  phraseId: "privet_lira",
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
    liveStatus: async () => ({
      available: false,
      sessionId: null,
      phase: "off",
      message: "",
    }),
    enableLive: async () => {
      throw new Error("unexpected live enable");
    },
    disableLive: async () => {
      throw new Error("unexpected live disable");
    },
    testAvailable: async () => false,
    test: async () => {
      throw new Error("unexpected detector test");
    },
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
  ({ wakeSamplesInSet } = await server.ssrLoadModule(
    "/src/v3/features/wake-samples/domain/samples.ts",
  ));
  ({ workspaceKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/workspace.ts",
  ));
  ({ default: Panel } = await server.ssrLoadModule(
    "/src/v3/features/wake-samples/presentation/WakeSamplesPanel.vue",
  ));
});
after(async () => await server?.close());

test("live wake requires an explicit enable and survives hiding or leaving the samples panel", async () => {
  const off = { available: true, sessionId: null, phase: "off", message: "" };
  const starting = { ...off, sessionId: 91, phase: "loading" };
  let starts = 0,
    stops = [],
    recordings = 0,
    checks = 0;
  const stop = deferred();
  const port = createWakeSamples(
    backend({
      list: async () => [sample],
      testAvailable: async () => true,
      liveStatus: async () => off,
      enableLive: async () => {
        starts++;
        return starting;
      },
      disableLive: async (id) => {
        stops.push(id);
        return stop.promise;
      },
      start: async () => {
        recordings++;
        return { sessionId: 5, durationMs: 5000 };
      },
      test: async () => {
        checks++;
      },
    }),
  );
  try {
    await port.load();
    assert.equal(starts, 0);
    await port.enableLive();
    await port.enableLive();
    assert.equal(starts, 1);
    await port.cancel();
    assert.equal(port.state.live.phase, "loading");
    assert.deepEqual(stops, []);
    await port.record(request);
    await port.test(sample.id);
    assert.equal(recordings, 0);
    assert.equal(checks, 0);
    const stopping = port.disableLive();
    await port.disableLive();
    assert.deepEqual(stops, [91]);
    stop.resolve(off);
    await stopping;
    assert.equal(port.state.live.phase, "off");
  } finally {
    port.dispose();
  }
  assert.deepEqual(stops, [91]);
});

test("a read started before enabling cannot overwrite the new live session", async () => {
  const oldRead = deferred();
  const off = { available: true, sessionId: null, phase: "off", message: "" };
  let reads = 0;
  const port = createWakeSamples(
    backend({
      liveStatus: async () => (++reads === 1 ? off : oldRead.promise),
      enableLive: async () => ({ ...off, sessionId: 17, phase: "loading" }),
    }),
  );
  try {
    await port.load();
    const stale = port.load();
    await port.enableLive();
    oldRead.resolve(off);
    await stale;
    assert.equal(port.state.live.sessionId, 17);
    assert.equal(port.state.live.phase, "loading");
  } finally {
    port.dispose();
  }
});

test("live controls explain the open microphone and allow stopping during model loading", async () => {
  const off = { available: true, sessionId: null, phase: "off", message: "" };
  const port = createWakeSamples(
    backend({
      liveStatus: async () => off,
      enableLive: async () => ({ ...off, sessionId: 2, phase: "loading" }),
    }),
  );
  const app = createSSRApp({ render: () => h(Panel) });
  app.use(WlConfig);
  app.provide(workspaceKey, {
    native: true,
    wakeSamples: port,
    state: { phase: "idle" },
  });
  try {
    await port.load();
    const initial = await renderToString(app);
    assert.match(initial, /микрофон остаётся открытым до отключения/);
    assert.match(initial, /Включить «Привет, Лира»/);
    await port.enableLive();
    const loading = await renderToString(app);
    assert.match(loading, /Готовим пробуждение/);
    assert.match(loading, /Выключить пробуждение/);
    assert.doesNotMatch(loading, /Включить «Привет, Лира»/);
  } finally {
    port.dispose();
  }
});

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

test("detector checks only run explicitly for saved Lira samples and retain no late result after leaving", async () => {
  const result = deferred();
  let checks = 0,
    starts = 0;
  const port = createWakeSamples(
    backend({
      testAvailable: async () => true,
      list: async () => [
        sample,
        { ...sample, id: "old", phraseId: "hey_fono" },
      ],
      test: async (id) => {
        assert.equal(id, sample.id);
        checks++;
        return result.promise;
      },
      start: async () => {
        starts++;
        return { sessionId: 9, durationMs: 5000 };
      },
    }),
  );
  await port.load();
  assert.equal(checks, 0);
  await port.test("old");
  await port.test("missing");
  assert.equal(checks, 0);
  const pending = port.test(sample.id);
  assert.equal(port.state.testingId, sample.id);
  await port.test(sample.id);
  await port.record(request);
  assert.equal(checks, 1);
  assert.equal(starts, 0);
  await port.cancel();
  result.resolve({
    operationId: "test-1",
    detected: true,
    proposals: 1,
    events: 1,
    processingMs: 30,
    durationMs: 5000,
  });
  await pending;
  assert.equal(port.state.testingId, null);
  assert.deepEqual({ ...port.state.testResults }, {});
  port.dispose();
});

test("a failed detector check is an error, never a successful not-detected result", async () => {
  const port = createWakeSamples(
    backend({
      testAvailable: async () => true,
      list: async () => [sample],
      test: async () => {
        throw new Error("check failed");
      },
    }),
  );
  await port.load();
  await port.test(sample.id);
  assert.equal(port.state.testingId, null);
  assert.equal(port.state.error, "check failed");
  assert.equal(port.state.testResults[sample.id], undefined);
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
    if (name === "get_lira_live_status")
      return { available: false, sessionId: null, phase: "off", message: "" };
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
      [
        "list_wake_samples",
        "wake_sample_test_available",
        "get_lira_live_status",
        "start_wake_sample",
        "cancel_wake_sample",
      ],
    );
    assert.deepEqual(calls[4][1], { sessionId: 81 });
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
  assert.match(html, /Привет, Лира/);
  assert.match(html, /iPhone/);
  assert.match(html, /Прежние образцы сюда не засчитываются/);
  assert.doesNotMatch(html, /Включить «Привет, Лира»/);
  assert.match(html, /не отправляет их на сервер/);
  assert.doesNotMatch(html, /wakeEnabled/);
  assert.match(html, /<button[^>]*disabled/);
  port.dispose();
});

test("the new phrase counter excludes both identified and unlabelled legacy recordings", () => {
  const legacy = { ...sample, id: "legacy", phraseId: "hey_fono" };
  const unlabelled = { ...sample, id: "v1" };
  delete unlabelled.phraseId;
  const rows = [sample, legacy, unlabelled];
  assert.deepEqual(
    wakeSamplesInSet(rows, request).map((s) => s.id),
    [sample.id],
  );
  assert.deepEqual(
    wakeSamplesInSet(rows, { ...request, phraseId: "hey_fono" }).map(
      (s) => s.id,
    ),
    ["legacy", "v1"],
  );
});

test("unsupported phrase language is rejected before microphone admission", async () => {
  let starts = 0;
  const port = createWakeSamples(
    backend({
      start: async () => {
        starts++;
      },
    }),
  );
  await port.record({ ...request, language: "en" });
  assert.equal(starts, 0);
  assert.equal(port.state.phase, "idle");
  assert.match(port.state.error, /Лира/);
  port.dispose();
});

test("phrase identity is captured with the request before a delayed start", async () => {
  const pending = deferred();
  let captured;
  const port = createWakeSamples(
    backend({
      start: (value) => {
        captured = value;
        return pending.promise;
      },
    }),
  );
  const draft = { ...request };
  const starting = port.record(draft);
  draft.phraseId = "hey_fono";
  assert.equal(captured.phraseId, "privet_lira");
  pending.resolve({ sessionId: 99, durationMs: 5000 });
  await starting;
  await port.cancel();
  port.dispose();
});
