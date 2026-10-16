import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer } from "vite";

let server;
const commands = [];

before(async () => {
  globalThis.window = {
    __TAURI_INTERNALS__: {
      invoke: async (command) => {
        commands.push(command);
        if (command === "get_desktop_snapshot")
          return { operation_id: 0, state: "idle", level: 0, last: null };
        if (command === "get_pending_dictation") return null;
        throw new Error(`Excluded or unexpected IPC: ${command}`);
      },
    },
  };
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    define: {
      __FONO_WAKE_WORD__: false,
      __FONO_EXPERIMENTAL_LIVE__: false,
    },
    server: { hmr: false, middlewareMode: true },
  });
});

after(async () => {
  await server?.close();
  delete globalThis.window;
});

test("default session refresh never invokes excluded live commands", async () => {
  const { createNativeSessionSync } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/sessionSync.ts",
  );
  const received = [];
  await createNativeSessionSync({
    operation: () => 0,
    snapshot: (value) => received.push(value.state),
    pending: () => {},
    live: (value) => assert.equal(value, null),
  }).refresh();
  assert.deepEqual(commands, ["get_desktop_snapshot", "get_pending_dictation"]);
  assert.deepEqual(received, ["idle"]);
});

test("frozen wake supports refresh/cancel and rejects capture without IPC", async () => {
  const { nativeWake } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/wake.ts",
  );
  const previousCommands = [...commands];
  const port = nativeWake({});
  await port.load();
  await port.cancel();
  for (const action of [
    port.begin,
    port.record,
    port.test,
    port.download,
    port.beginValidation,
    () => port.validate("positive"),
  ])
    await assert.rejects(action, /Временно недоступно/);
  assert.deepEqual(commands, previousCommands);
});

test("default build ignores live events and keeps wake disabling on the settings port", async () => {
  const { nativeDictation } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/dictation.ts",
  );
  const { nativeSettings } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/settings.ts",
  );
  const state = {
    preferences: { dictationMode: "live", wakeEnabled: false },
    pending: {},
    live: null,
  };
  const patches = [];
  const context = { state, save: async (patch) => patches.push(patch) };
  const previousCommands = [...commands];
  const dictation = nativeDictation(context);
  dictation.live({ session_id: "1", phase: "listening", revision: 1 });
  assert.equal(state.live, null);
  await dictation.port.resumeInsertion();
  await nativeSettings(context).toggle("wakeEnabled", false);
  assert.deepEqual(patches, [{ wakeEnabled: false }]);
  assert.deepEqual(commands, previousCommands);
});
