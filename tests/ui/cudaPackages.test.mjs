import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";

let server, createNative, createMock, progress;
before(async () => {
  server = await createServer({
    appType: "custom",
    server: { middlewareMode: true, hmr: false },
  });
  ({ createNativeCudaPackages: createNative } = await server.ssrLoadModule(
    "/src/v3/features/cuda-packages/infrastructure/nativeCudaPackages.ts",
  ));
  ({ createMockCudaPackages: createMock } = await server.ssrLoadModule(
    "/src/v3/features/cuda-packages/infrastructure/mockCudaPackages.ts",
  ));
  ({ cudaPackageProgress: progress } = await server.ssrLoadModule(
    "/src/v3/shared/domain/cudaPackages.ts",
  ));
});
after(async () => await server?.close());
const flush = () => new Promise((resolve) => setImmediate(resolve));
const snapshot = (phase, extra = {}) => ({
  phase,
  operation_id: null,
  downloaded_bytes: 0,
  total_bytes: null,
  installed_revision: null,
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

test("startup only reads status; all transfers require an explicit action", async () => {
  const calls = [];
  const port = createNative(async (command) => {
    calls.push(command);
    return snapshot("not_installed");
  });
  try {
    await flush();
    assert.deepEqual(calls, ["get_cuda_package_status"]);
    assert.equal(port.state.phase, "not_installed");
    assert.equal(port.state.totalBytes, null);
  } finally {
    port.dispose();
  }
});

test("late startup success and failure cannot overwrite a newer install", async () => {
  for (const reject of [false, true]) {
    const initial = deferred();
    const completion = deferred();
    let reads = 0;
    let status = snapshot("installed", {
      operation_id: "new",
      installed_revision: 3,
    });
    const port = createNative(
      async (command) => {
        if (command === "get_cuda_package_status") {
          if (!reads++) return initial.promise;
          return status;
        }
        return completion.promise;
      },
      async () => {},
      () => true,
      () => "new",
    );
    try {
      const install = port.install();
      if (reject) initial.reject(new Error("stale status failure"));
      else initial.resolve(snapshot("not_installed"));
      await flush();
      assert.equal(port.state.phase, "checking");
      assert.equal(port.state.operationId, "new");
      completion.resolve(status);
      await install;
      assert.equal(port.state.phase, "installed");
      assert.equal(port.state.installedRevision, 3);
    } finally {
      port.dispose();
    }
  }
});

test("single flight and cancellation use the exact active operation id", async () => {
  const download = deferred();
  let status = snapshot("not_installed");
  const calls = [];
  let activated = 0;
  const port = createNative(
    async (command, args) => {
      calls.push({ command, args });
      if (command === "get_cuda_package_status") return status;
      if (command === "install_cuda_package") {
        assert.equal(args.operationId, "one");
        return download.promise;
      }
      assert.equal(args.operationId, "one");
      status = snapshot("cancelling", { operation_id: "one" });
      return status;
    },
    async () => {
      activated++;
    },
    () => true,
    () => "one",
  );
  try {
    await flush();
    const installation = port.install();
    await assert.rejects(port.install(), /уже выполняется/);
    await port.cancel();
    assert.equal(port.state.phase, "cancelling");
    await assert.rejects(port.install(), /уже выполняется/);
    // A previous install's terminal response may arrive after cancel. Only
    // the authoritative fresh status may finalize this generation.
    status = snapshot("cancelled", { operation_id: "one" });
    download.resolve(snapshot("installed", { operation_id: "one" }));
    await installation;
    assert.equal(port.state.phase, "cancelled");
    assert.equal(activated, 0);
    assert.equal(
      calls.filter((c) => c.command === "install_cuda_package").length,
      1,
    );
  } finally {
    port.dispose();
  }
});

test("a failed native install refreshes the authoritative retryable error", async () => {
  const status = snapshot("error", {
    operation_id: "retry",
    message: "Пакет пока не опубликован",
  });
  let reads = 0;
  const port = createNative(
    async (command) => {
      if (command === "get_cuda_package_status")
        return reads++ ? status : snapshot("not_installed");
      throw new Error("HTTP not available");
    },
    async () => {},
    () => true,
    () => "retry",
  );
  try {
    await flush();
    await assert.rejects(port.install(), /not available/);
    assert.equal(port.state.phase, "error");
    assert.equal(port.state.message, status.message);
  } finally {
    port.dispose();
  }
});

test("unavailable CPU profile cannot start a package download", async () => {
  const calls = [];
  const port = createNative(async (command) => {
    calls.push(command);
    return snapshot("unavailable");
  });
  try {
    await flush();
    await assert.rejects(port.install(), /недоступна/);
    assert.deepEqual(calls, ["get_cuda_package_status"]);
  } finally {
    port.dispose();
  }
});

test("hidden/disposed native renderer neither polls nor cancels user installation", async () => {
  let visible = false;
  const completion = deferred();
  const calls = [];
  const port = createNative(
    async (command) => {
      calls.push(command);
      if (command === "install_cuda_package") return completion.promise;
      return snapshot("not_installed");
    },
    async () => {},
    () => visible,
    () => "visible",
  );
  await flush();
  await port.refresh();
  assert.equal(calls.length, 0);
  visible = true;
  await port.refresh();
  const installation = port.install();
  port.dispose();
  completion.resolve(snapshot("installed", { operation_id: "visible" }));
  await installation;
  assert.deepEqual(calls, ["get_cuda_package_status", "install_cuda_package"]);
  assert.equal(
    port.state.phase,
    "checking",
    "A disposed view cannot publish the late result",
  );
});

test("browser demonstration cancellation never becomes installed and is retryable", async () => {
  const port = createMock();
  try {
    const first = port.install();
    await port.cancel();
    await first;
    assert.equal(port.state.phase, "cancelled");
    assert.equal(port.state.installedRevision, null);
    await port.install();
    assert.equal(port.state.phase, "installed");
    assert.match(port.state.message, /не загружались/);
  } finally {
    port.dispose();
  }
});

test("unknown size stays indeterminate and download progress is bounded", () => {
  assert.equal(progress({ downloadedBytes: 10, totalBytes: null }), null);
  assert.equal(progress({ downloadedBytes: 1000, totalBytes: 100 }), 100);
  assert.equal(progress({ downloadedBytes: -1, totalBytes: 100 }), 0);
});
