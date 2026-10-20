import assert from "node:assert/strict";
import { before, beforeEach, after, test } from "node:test";
import { createServer } from "vite";

let server,
  createCoalescedRead,
  createContext,
  defaults,
  rawDefaults,
  call,
  waitForDesktopReady;
let invoke, calls;
const flush = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
before(async () => {
  globalThis.window = {
    __TAURI_INTERNALS__: { invoke: (...args) => invoke(...args) },
  };
  server = await createServer({
    appType: "custom",
    server: { middlewareMode: true, hmr: false },
  });
  ({ createCoalescedRead } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/coalescedRead.ts",
  ));
  ({ createNativeContext: createContext } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/context.ts",
  ));
  ({ call } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/ipc.ts",
  ));
  ({ waitForDesktopReady } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/startupReady.ts",
  ));
  ({ defaults } = await server.ssrLoadModule(
    "/src/v3/features/preferences/domain/preferences.ts",
  ));
  ({ DEFAULT_SETTINGS: rawDefaults } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/ipcTypes.ts",
  ));
});
beforeEach(() => {
  calls = [];
  invoke = async (name) => {
    calls.push(name);
    if (name === "get_settings") return structuredClone(rawDefaults);
    if (name === "get_dictation_history" || name === "list_stt_models")
      return [];
    if (name === "get_local_transcription_service_snapshot")
      return service("initial", false);
    if (name === "get_stt_memory_status") return gpu("resident");
  };
});
after(async () => {
  await server?.close();
  delete globalThis.window;
});

test("desktop startup waits for native setup with one subscription and one read", async () => {
  let ready,
    released = 0,
    reads = 0,
    mounted = false;
  const pending = waitForDesktopReady({
    listen: async (callback) => {
      ready = callback;
      return () => released++;
    },
    read: async () => {
      reads++;
      return false;
    },
  }).then(() => {
    mounted = true;
  });
  await flush();
  assert.equal(mounted, false);
  assert.equal(reads, 1);
  ready();
  await pending;
  assert.equal(mounted, true);
  assert.equal(released, 1);
});

test("desktop startup handles already ready and the subscribe/read race", async () => {
  for (const alreadyReady of [true, false]) {
    let release = 0,
      ready;
    await waitForDesktopReady({
      listen: async (callback) => {
        ready = callback;
        return () => release++;
      },
      read: async () => {
        if (!alreadyReady) ready();
        return alreadyReady;
      },
    });
    assert.equal(release, 1);
  }
});

test("desktop startup timeout cleans up even a late listener", async () => {
  let release = 0;
  const subscription = deferred();
  await assert.rejects(
    waitForDesktopReady(
      {
        listen: () => subscription.promise,
        read: async () => false,
      },
      15,
    ),
    /timed out/,
  );
  subscription.resolve(() => release++);
  await flush();
  assert.equal(release, 1);
});
function state() {
  return {
    preferences: { ...defaults },
    profiles: [],
    models: [],
    history: [],
    applications: [],
    jobs: [],
    last: {
      entry: null,
      variant: "result",
      draft: "",
      edited: false,
      undo: null,
    },
    phase: "idle",
    audioLevel: 0,
    elapsed: 0,
    error: "",
    pending: {},
    logs: [],
  };
}
const gpu = (residency) => ({
  mode: "resident",
  residency,
  monitoring: "disabled",
  message: "",
});
const service = (address, enabled) => ({
  address,
  enabled,
  error: null,
  queue: { jobs: [] },
  history: { jobs: [] },
});
const historyEntry = (id) => ({
  id,
  text: "retained result",
  created_at: "2026-10-17T12:00:00Z",
  device: null,
});

test("a burst shares one in-flight read and waits for exactly one latest follow-up", async () => {
  const reads = [],
    applied = [];
  const reader = createCoalescedRead(
    () => {
      const pending = deferred();
      reads.push(pending);
      return pending.promise;
    },
    (value) => applied.push(value),
  );
  const first = reader.refresh();
  await flush();
  assert.equal(reads.length, 1);
  const burst = Array.from({ length: 20 }, () => reader.refresh());
  assert.ok(burst.every((promise) => promise === first));
  let settled = false;
  void first.then(() => (settled = true));
  reads[0].resolve("obsolete");
  await flush();
  assert.deepEqual(applied, []);
  assert.equal(reads.length, 2);
  assert.equal(settled, false, "callers must wait for the post-mutation read");
  reads[1].resolve("latest");
  await Promise.all([first, ...burst]);
  assert.deepEqual(applied, ["latest"]);
  assert.equal(reads.length, 2);
});

test("an external push retires an older reply without scheduling another IPC read", async () => {
  const old = deferred();
  let reads = 0;
  const applied = [];
  const reader = createCoalescedRead(
    () => {
      reads++;
      return old.promise;
    },
    (value) => applied.push(value),
  );
  const task = reader.refresh();
  await flush();
  reader.invalidate();
  old.resolve("old");
  await task;
  assert.equal(reads, 1);
  assert.deepEqual(applied, []);
});

test("a microtask refresh at drain completion cannot be lost in a Promise.finally gap", async () => {
  let reader, successor;
  const applied = [],
    reads = [];
  reader = createCoalescedRead(
    () => {
      const next = deferred();
      reads.push(next);
      return next.promise;
    },
    (value) => {
      applied.push(value);
      if (value === "first")
        queueMicrotask(() => (successor = reader.refresh()));
    },
  );
  const first = reader.refresh();
  await flush();
  reads[0].resolve("first");
  await first;
  await flush();
  assert.equal(reads.length, 2);
  reads[1].resolve("second");
  await successor;
  assert.deepEqual(applied, ["first", "second"]);
});

test("stale failures do not reject a queued latest read, and current failures allow retry", async () => {
  const old = deferred(),
    next = deferred();
  let count = 0;
  const applied = [];
  const reader = createCoalescedRead(
    () => (++count === 1 ? old.promise : next.promise),
    (value) => applied.push(value),
  );
  const first = reader.refresh();
  await flush();
  const latest = reader.refresh();
  old.reject(new Error("obsolete failure"));
  await flush();
  next.resolve("latest");
  await Promise.all([first, latest]);
  assert.deepEqual(applied, ["latest"]);
  let fail = true;
  const retry = createCoalescedRead(
    async () => {
      if (fail) throw new Error("current failure");
      return "recovered";
    },
    (value) => applied.push(value),
  );
  await assert.rejects(retry.refresh(), /current failure/);
  fail = false;
  await retry.refresh();
  assert.equal(applied.at(-1), "recovered");
});

test("hidden and disposed readers emit no IPC and cannot apply late results", async () => {
  let visible = false;
  const reads = [],
    applied = [];
  const reader = createCoalescedRead(
    () => {
      const next = deferred();
      reads.push(next);
      return next.promise;
    },
    (value) => applied.push(value),
    () => visible,
  );
  await reader.refresh();
  assert.equal(reads.length, 0);
  visible = true;
  const old = reader.refresh();
  await flush();
  visible = false;
  reads[0].resolve("hidden");
  await old;
  assert.deepEqual(applied, []);
  visible = true;
  const disposed = reader.refresh();
  await flush();
  reader.dispose();
  reads[1].resolve("disposed");
  await disposed;
  await reader.refresh();
  assert.equal(reads.length, 2);
  assert.deepEqual(applied, []);
});

test("an older history read cannot restore a deleted row or seed Last after deletion", async () => {
  const old = deferred(),
    latest = deferred();
  let historyReads = 0;
  const baseline = invoke;
  invoke = async (name, args) => {
    if (name === "get_dictation_history") {
      calls.push(name);
      return ++historyReads === 1 ? old.promise : latest.promise;
    }
    return baseline(name, args);
  };
  const data = state(),
    ctx = createContext(data);
  const first = ctx.readHistory();
  await flush();
  await call("delete_dictation_history_entry", { id: "deleted" });
  const afterDelete = ctx.readHistory();
  old.resolve([historyEntry("deleted")]);
  await flush();
  assert.deepEqual(data.history, []);
  assert.equal(data.last.entry, null);
  assert.equal(historyReads, 2);
  latest.resolve([]);
  await Promise.all([first, afterDelete]);
  assert.deepEqual(data.history, []);
  assert.equal(data.last.entry, null);
  ctx.dispose();
});

test("service event bursts never let an old enabled flag overwrite a newer snapshot", async () => {
  const old = deferred(),
    latest = deferred();
  let serviceReads = 0;
  invoke = async (name) => {
    assert.equal(name, "get_local_transcription_service_snapshot");
    return ++serviceReads === 1 ? old.promise : latest.promise;
  };
  const data = state(),
    ctx = createContext(data);
  const first = ctx.readService();
  await flush();
  const burst = Array.from({ length: 12 }, () => ctx.readService());
  old.resolve(service("stale-address", true));
  await flush();
  assert.equal(data.serviceAddress, undefined);
  latest.resolve(service("latest-address", false));
  await Promise.all([first, ...burst]);
  assert.equal(data.serviceAddress, "latest-address");
  assert.equal(data.preferences.serviceEnabled, false);
  assert.equal(serviceReads, 2);
  ctx.dispose();
});

test("GPU push owns state over both stale successful and stale failed diagnostic replies", async () => {
  for (const failure of [false, true]) {
    const old = deferred();
    calls = [];
    invoke = async (name) => {
      calls.push(name);
      return old.promise;
    };
    const data = state(),
      ctx = createContext(data);
    const read = ctx.readGpuMemory();
    await flush();
    const pushed = gpu("active");
    ctx.acceptGpuMemory(pushed);
    if (failure) old.reject(new Error("outdated diagnostic failure"));
    else old.resolve(gpu("unloaded"));
    await read;
    assert.deepEqual(data.gpuMemory, pushed);
    assert.equal(data.gpuMemoryError, "");
    assert.deepEqual(calls, ["get_stt_memory_status"]);
    ctx.dispose();
  }
});

test("model bursts publish only the final catalogue and do not lose installed status", async () => {
  const old = deferred(),
    latest = deferred();
  let modelReads = 0;
  invoke = async () => (++modelReads === 1 ? old.promise : latest.promise);
  const data = state(),
    ctx = createContext(data);
  const first = ctx.readModels();
  await flush();
  const afterRemoval = ctx.readModels();
  const info = (variant, path) => ({
    id: { engine: "whisper", variant },
    filename: variant + ".bin",
    local_path: path,
    bytes: 10,
    capabilities: {},
  });
  old.resolve([info("tiny", "old")]);
  await flush();
  assert.deepEqual(data.models, []);
  latest.resolve([info("base", "new")]);
  await Promise.all([first, afterRemoval]);
  assert.equal(data.models[0].id, "base");
  assert.equal(data.models[0].status, "installed");
  assert.equal(modelReads, 2);
  ctx.dispose();
});

test("late custom model metadata cannot overwrite newer settings, while Settings return values stay intact", async () => {
  const metadata = deferred();
  let settingsReads = 0;
  const oldSettings = {
    ...structuredClone(rawDefaults),
    hotkey_mode: "hold",
    whisper_model_path: "old-custom.bin",
    launch_apps: [{ name: "old", exe_path: "old", aliases: ["old"] }],
  };
  const newSettings = {
    ...structuredClone(rawDefaults),
    hotkey_mode: "toggle",
    launch_apps: [{ name: "new", exe_path: "new", aliases: ["new"] }],
  };
  invoke = async (name) => {
    if (name === "get_settings")
      return ++settingsReads === 1 ? oldSettings : newSettings;
    assert.equal(name, "get_selected_model_metadata");
    return metadata.promise;
  };
  const data = state(),
    ctx = createContext(data);
  const first = ctx.readSettings();
  await flush();
  assert.deepEqual(await ctx.readSettings(), newSettings);
  metadata.resolve({
    path: "old-custom.bin",
    filename: "old.bin",
    bytes: 1000,
  });
  assert.deepEqual(await first, oldSettings);
  assert.equal(data.preferences.hotkeyMode, "toggle");
  assert.equal(data.applications[0].name, "new");
  assert.ok(!data.models.some((model) => model.id === "old-custom.bin"));
  ctx.dispose();
});

test("context disposal invalidates active history reads and blocks every later snapshot read", async () => {
  const old = deferred();
  invoke = async (name) => {
    calls.push(name);
    return old.promise;
  };
  const data = state(),
    ctx = createContext(data);
  const task = ctx.readHistory();
  await flush();
  ctx.dispose();
  old.resolve([historyEntry("retired")]);
  await task;
  await Promise.all([
    ctx.readHistory(),
    ctx.readService(),
    ctx.readModels(),
    ctx.readGpuMemory(),
    ctx.refresh(),
  ]);
  assert.deepEqual(data.history, []);
  assert.deepEqual(calls, ["get_dictation_history"]);
});
