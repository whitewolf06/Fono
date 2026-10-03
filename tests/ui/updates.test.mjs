import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";

let server, createMockUpdates, createNativeUpdates, downloadPercent;
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
