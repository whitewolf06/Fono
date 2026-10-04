import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";
import { reactive } from "vue";
import {
  createToggleHarness,
  flushChanges,
} from "./preferenceToggleHarness.mjs";

let server,
  createMockUpdates,
  createNativeUpdates,
  downloadPercent,
  mountUpdates;
before(async () => {
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { hmr: false, middlewareMode: true },
  });
  ({ createMockUpdates } = await server.ssrLoadModule(
    "/src/v3/features/updates/infrastructure/mockUpdates.ts",
  ));
  ({ createNativeUpdates } = await server.ssrLoadModule(
    "/src/v3/features/updates/infrastructure/nativeUpdates.ts",
  ));
  ({ downloadPercent } = await server.ssrLoadModule(
    "/src/v3/shared/domain/updates.ts",
  ));
  const { workspaceKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/workspace.ts",
  );
  const { interactionKey } = await server.ssrLoadModule(
    "/src/v3/shared/application/interaction.ts",
  );
  mountUpdates = await createToggleHarness(
    server,
    workspaceKey,
    interactionKey,
    {
      componentPath: "src/v3/features/updates/presentation/UpdatesPanel.vue",
      props: { unsaved: false },
      imports: {
        "../../../shared/domain/updates": "/src/v3/shared/domain/updates.ts",
      },
    },
  );
});
after(async () => await server?.close());

test("checking availability does not implicitly install; duplicate checks are rejected", async () => {
  const port = createMockUpdates("0.5.21");
  const check = port.check();
  await assert.rejects(port.check(), /уже выполняется/);
  await check;
  assert.equal(port.state.phase, "available");
  assert.equal(port.state.downloadedBytes, 0);
  assert.equal(port.state.nextVersion, "0.5.22");
  port.dispose();
});
test("cancelled download never reaches installation and remains retryable", async () => {
  const port = createMockUpdates("0.5.21");
  await port.check();
  const download = port.install();
  await port.cancel();
  await download;
  assert.equal(port.state.phase, "available");
  assert.equal(port.state.downloadedBytes, 0);
  await port.install();
  assert.equal(port.state.phase, "up_to_date");
  port.dispose();
});
test("native startup only reads status; opting into checks never starts installation", async () => {
  const commands = [];
  const snapshot = {
    phase: "not_configured",
    currentVersion: "0.5.21",
    downloadedBytes: 0,
    checksEnabled: false,
    message: "Канал не настроен",
  };
  const port = createNativeUpdates("0.5.21", async (command, args) => {
    commands.push(command);
    if (command === "set_update_checks_enabled")
      snapshot.checksEnabled = args.enabled;
    return { ...snapshot };
  });
  try {
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.deepEqual(commands, ["get_update_status"]);
    await port.setChecksEnabled(true);
    assert.equal(port.state.checksEnabled, true);
    assert.ok(!commands.includes("install_update"));
    assert.ok(!commands.includes("check_for_updates"));
  } finally {
    port.dispose();
  }
});
test("unknown length stays indeterminate and reported progress is bounded", () => {
  assert.equal(downloadPercent({ downloadedBytes: 100 }), null);
  assert.equal(
    downloadPercent({ downloadedBytes: 1000, totalBytes: 100 }),
    100,
  );
  assert.equal(downloadPercent({ downloadedBytes: -1, totalBytes: 100 }), 0);
});

const snapshot = (phase, extra = {}) => ({
  phase,
  currentVersion: "0.6.10",
  downloadedBytes: 0,
  checksEnabled: false,
  message: phase,
  ...extra,
});
function deferred() {
  let resolve, reject;
  const promise = new Promise((accept, decline) => {
    resolve = accept;
    reject = decline;
  });
  return { promise, resolve, reject };
}
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

test("actual WhiteUI auto-check switch restores the confirmed value and focus after failed save", async () => {
  for (const initial of [false, true]) {
    const status = reactive(snapshot("idle", { checksEnabled: initial }));
    const save = deferred();
    let writes = 0;
    const control = mountUpdates(
      "auto-check",
      {
        preferences: {},
        phase: "idle",
        pendingDictation: null,
        updates: {
          state: status,
          setChecksEnabled: () => {
            writes++;
            return save.promise;
          },
        },
      },
      async () => false,
    );
    try {
      control.assertState(initial);
      await control.change(!initial);
      await flushChanges();
      control.assertState(initial);
      assert.equal(control.input.disabled, true);
      await control.change(!initial);
      assert.equal(writes, 1, "A pending save must not start another save");
      control.assertState(initial);
      save.reject(new Error("Cannot save auto-check preference"));
      await flushChanges();
      assert.equal(status.checksEnabled, initial);
      assert.equal(control.input.disabled, false);
      control.assertState(initial);
    } finally {
      control.dispose();
    }
  }
});

test("actual WhiteUI auto-check switch applies successful save without replacing the focus target", async () => {
  const status = reactive(snapshot("idle"));
  const save = deferred();
  const control = mountUpdates(
    "auto-check",
    {
      preferences: {},
      phase: "idle",
      pendingDictation: null,
      updates: {
        state: status,
        async setChecksEnabled(value) {
          await save.promise;
          status.checksEnabled = value;
        },
      },
    },
    async () => false,
  );
  try {
    await control.change(true);
    await flushChanges();
    control.assertState(false);
    assert.equal(control.input.disabled, true);
    save.resolve();
    await flushChanges();
    assert.equal(control.input.disabled, false);
    control.assertState(true);
  } finally {
    control.dispose();
  }
});

test("actual WhiteUI auto-check switch cannot enable an unconfigured channel", async () => {
  let writes = 0;
  const status = reactive(snapshot("not_configured"));
  const control = mountUpdates(
    "auto-check",
    {
      preferences: {},
      phase: "idle",
      pendingDictation: null,
      updates: {
        state: status,
        setChecksEnabled: async () => {
          writes++;
        },
      },
    },
    async () => false,
  );
  try {
    assert.equal(control.input.disabled, true);
    await control.change(true);
    await flushChanges();
    assert.equal(writes, 0);
    control.assertState(false);
  } finally {
    control.dispose();
  }
});

test("delayed startup success and failure cannot erase a completed update check", async () => {
  for (const outcome of ["success", "error"]) {
    const oldRead = deferred();
    let reads = 0;
    const available = snapshot("available", { nextVersion: "0.6.11" });
    const port = createNativeUpdates("0.6.10", async (command) => {
      if (command === "get_update_status" && ++reads === 1)
        return oldRead.promise;
      return available;
    });
    try {
      await port.check();
      assert.equal(port.state.phase, "available");
      if (outcome === "success") oldRead.resolve(snapshot("idle"));
      else oldRead.reject(new Error("Old startup failed"));
      await flush();
      assert.equal(port.state.phase, "available");
      assert.equal(port.state.nextVersion, "0.6.11");
      assert.equal(port.state.message, "available");
    } finally {
      port.dispose();
    }
  }
});

test("poll started before an action cannot erase that action's newer settings", async () => {
  const oldPoll = deferred();
  let reads = 0;
  let current = snapshot("idle");
  const port = createNativeUpdates("0.6.10", async (command, args) => {
    if (command === "get_update_status") {
      if (++reads === 2) return oldPoll.promise;
      return { ...current };
    }
    current = snapshot("idle", { checksEnabled: args.enabled });
    return { ...current };
  });
  try {
    await flush();
    const poll = port.refresh();
    await port.setChecksEnabled(true);
    oldPoll.resolve(snapshot("idle", { checksEnabled: false }));
    await poll;
    assert.equal(port.state.checksEnabled, true);
  } finally {
    port.dispose();
  }
});

test("download polls show progress; polls crossing action completion cannot restore old progress", async () => {
  const install = deferred();
  const latePoll = deferred();
  let current = snapshot("available", { nextVersion: "0.6.11" });
  let delayNextRead = false;
  let nextReadSnapshot = null;
  const port = createNativeUpdates("0.6.10", async (command) => {
    if (command === "install_update") {
      current = snapshot("downloading", {
        nextVersion: "0.6.11",
        downloadedBytes: 40,
        totalBytes: 100,
      });
      return install.promise;
    }
    if (delayNextRead) {
      delayNextRead = false;
      return latePoll.promise;
    }
    if (nextReadSnapshot) {
      const value = nextReadSnapshot;
      nextReadSnapshot = null;
      return value;
    }
    return { ...current };
  });
  try {
    await flush();
    const installing = port.install();
    await port.refresh();
    assert.equal(port.state.phase, "downloading");
    assert.equal(port.state.downloadedBytes, 40);
    nextReadSnapshot = snapshot("idle");
    await port.refresh();
    assert.equal(port.state.phase, "downloading");
    assert.equal(port.state.downloadedBytes, 40);
    delayNextRead = true;
    const poll = port.refresh();
    current = snapshot("up_to_date");
    install.resolve({ ...current });
    await installing;
    latePoll.resolve(
      snapshot("downloading", { downloadedBytes: 20, totalBytes: 100 }),
    );
    await poll;
    assert.equal(port.state.phase, "up_to_date");
    assert.equal(port.state.downloadedBytes, 0);
  } finally {
    port.dispose();
  }
});

test("post-action status failure preserves successful action and the original action error", async () => {
  for (const failAction of [false, true]) {
    let reads = 0;
    const port = createNativeUpdates("0.6.10", async (command) => {
      if (command === "get_update_status") {
        if (++reads === 1) return snapshot("idle");
        throw new Error("Status refresh failed");
      }
      if (failAction) throw new Error("Update check failed");
      return snapshot("available", { nextVersion: "0.6.11" });
    });
    try {
      await flush();
      if (failAction) await assert.rejects(port.check(), /Update check failed/);
      else {
        await port.check();
        assert.equal(port.state.phase, "available");
        assert.equal(port.state.nextVersion, "0.6.11");
      }
    } finally {
      port.dispose();
    }
  }
});

test("cancel wins over a late installation response while older work settles", async () => {
  const install = deferred();
  let current = snapshot("available", { nextVersion: "0.6.11" });
  const port = createNativeUpdates("0.6.10", async (command) => {
    if (command === "install_update") {
      current = snapshot("downloading", { nextVersion: "0.6.11" });
      return install.promise;
    }
    if (command === "cancel_update_download")
      current = snapshot("available", { nextVersion: "0.6.11" });
    return { ...current };
  });
  try {
    await flush();
    const installing = port.install();
    await port.cancel();
    assert.equal(port.state.phase, "available");
    install.resolve(snapshot("error", { message: "Old download stopped" }));
    await installing;
    assert.equal(port.state.phase, "available");
    assert.equal(port.state.nextVersion, "0.6.11");
  } finally {
    port.dispose();
  }
});
