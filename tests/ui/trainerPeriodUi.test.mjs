import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer as createTcpServer } from "node:net";
import { createServer } from "vite";
import { nextTick, ref, watch } from "vue";

let server, ui;
before(async () => {
  const socket = createTcpServer();
  await new Promise((resolve, reject) => {
    socket.once("error", reject);
    socket.listen(0, "127.0.0.1", resolve);
  });
  const port = socket.address().port;
  await new Promise((resolve) => socket.close(resolve));
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    optimizeDeps: { noDiscovery: true, include: [] },
    server: { hmr: { host: "127.0.0.1", port }, middlewareMode: true },
  });
  ui = await server.ssrLoadModule(
    "/src/v3/features/trainer/application/periodReportController.ts",
  );
});
after(async () => server?.close());

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
const tick = async () => {
  for (let count = 0; count < 8; count++) await Promise.resolve();
};
const report = (fingerprint, options = {}) => ({
  snapshot: { fingerprint, days: 7 },
  stale: false,
  aiStale: true,
  aiSummary: null,
  aiConfiguration: { dataScope: "metrics_only" },
  aiDisabledReason: null,
  ...options,
});

test("a slow previous period cannot replace the selected period", async () => {
  const seven = deferred();
  const thirty = deferred();
  const state = ui.initialPeriodReportState();
  const controller = ui.createPeriodReportController(
    {
      getReport: (days) => (days === 7 ? seven.promise : thirty.promise),
      analyzePeriod: () => assert.fail("loading a period must not invoke AI"),
      cancelPeriod: async () => {},
    },
    state,
  );
  const oldRead = controller.load();
  await tick();
  const newRead = controller.setPeriod(30);
  await tick();
  const selected = report("thirty", {
    snapshot: { fingerprint: "thirty", days: 30 },
  });
  thirty.resolve(selected);
  await newRead;
  seven.resolve(report("seven"));
  await oldRead;
  assert.equal(state.days, 30);
  assert.equal(state.report, selected);
  assert.equal(state.loading, false);
});

test("an ordinary source read retains the snapshot while an explicit refresh rebuilds it", async () => {
  const rebuilt = deferred();
  const calls = [];
  const old = report("old", { stale: true });
  const state = { ...ui.initialPeriodReportState(), report: old };
  const controller = ui.createPeriodReportController(
    {
      getReport: async (days, refresh) => {
        calls.push([days, refresh]);
        return refresh ? rebuilt.promise : old;
      },
      analyzePeriod: () => assert.fail("source updates must not invoke AI"),
      cancelPeriod: async () => {},
    },
    state,
  );
  await controller.load();
  assert.equal(state.report, old);
  const refresh = controller.load(true);
  await tick();
  assert.equal(state.report, old);
  const fresh = report("fresh");
  rebuilt.resolve(fresh);
  await refresh;
  assert.deepEqual(calls, [
    [7, false],
    [7, true],
  ]);
  assert.equal(state.report, fresh);
});

test("AI stays numeric by default and does not repeat a current summary with the same scope", async () => {
  const calls = [];
  const base = report("source", {
    aiStale: false,
    aiSummary: { dataScope: "metrics_only" },
    aiConfiguration: { dataScope: "findings" },
  });
  assert.match(ui.periodAiDisabledReason(base, false), /уже разобран/);
  assert.equal(ui.periodAiDisabledReason(base, true), null);
  const state = { ...ui.initialPeriodReportState(), report: base };
  const controller = ui.createPeriodReportController(
    {
      getReport: async () => base,
      analyzePeriod: async (request) => {
        calls.push(request);
        return report("source");
      },
      cancelPeriod: async () => {},
    },
    state,
  );
  await controller.analyze(false);
  assert.equal(calls.length, 0);
  await controller.analyze(true);
  assert.deepEqual(calls, [
    { days: 7, fingerprint: "source", shareText: true },
  ]);
  state.report = report("next");
  await controller.analyze(false);
  assert.equal(calls[1].shareText, false);
});

test("changed sources cancel AI before reading freshness and discard its late result", async () => {
  const answer = deferred();
  const cancel = deferred();
  let reads = 0;
  const fresh = report("changed");
  const state = { ...ui.initialPeriodReportState(), report: report("source") };
  const controller = ui.createPeriodReportController(
    {
      getReport: async () => {
        reads++;
        return fresh;
      },
      analyzePeriod: () => answer.promise,
      cancelPeriod: () => cancel.promise,
    },
    state,
  );
  const analysis = controller.analyze(false);
  await tick();
  const reload = controller.load();
  await tick();
  assert.equal(reads, 0);
  assert.equal(state.cancelling, true);
  answer.resolve(report("late-answer"));
  await analysis;
  cancel.resolve();
  await reload;
  assert.equal(state.report, fresh);
  assert.equal(state.analyzing, false);
  assert.equal(state.cancelling, false);
});

test("clear removes the visible report and ignores a late failed AI response", async () => {
  const answer = deferred();
  let cancels = 0;
  const state = { ...ui.initialPeriodReportState(), report: report("source") };
  const controller = ui.createPeriodReportController(
    {
      getReport: async () => report("unused"),
      analyzePeriod: () => answer.promise,
      cancelPeriod: async () => {
        cancels++;
      },
    },
    state,
  );
  const analysis = controller.analyze(false);
  await tick();
  await controller.clear();
  answer.reject(new Error("late provider error"));
  await analysis;
  assert.equal(cancels, 1);
  assert.equal(state.report, null);
  assert.equal(state.error, "");
});

test("unmount cancels the operation and cannot publish its late response", async () => {
  const answer = deferred();
  const base = report("source");
  const state = { ...ui.initialPeriodReportState(), report: base };
  let cancels = 0;
  const controller = ui.createPeriodReportController(
    {
      getReport: async () => base,
      analyzePeriod: () => answer.promise,
      cancelPeriod: async () => {
        cancels++;
      },
    },
    state,
  );
  const analysis = controller.analyze(false);
  await tick();
  await controller.dispose();
  answer.resolve(report("late-answer"));
  await analysis;
  assert.equal(cancels, 1);
  assert.equal(state.report, base);
});

test("a failed refresh keeps the last snapshot available", async () => {
  const base = report("source");
  const state = { ...ui.initialPeriodReportState(), report: base };
  const controller = ui.createPeriodReportController(
    {
      getReport: async () => {
        throw new Error("storage unavailable");
      },
      analyzePeriod: () => assert.fail("failed reads must not invoke AI"),
      cancelPeriod: async () => {},
    },
    state,
  );
  await controller.load(true);
  assert.equal(state.report, base);
  assert.equal(state.error, "storage unavailable");
  assert.equal(state.loading, false);
});

test("a failed cancellation blocks the read but an explicit retry can recover", async () => {
  const answer = deferred();
  const state = { ...ui.initialPeriodReportState(), report: report("source") };
  let reads = 0;
  let cancellations = 0;
  const controller = ui.createPeriodReportController(
    {
      getReport: async () => {
        reads++;
        return report("recovered");
      },
      analyzePeriod: () => answer.promise,
      cancelPeriod: async () => {
        cancellations++;
        if (cancellations === 1) throw new Error("cancel unavailable");
      },
    },
    state,
  );
  const analysis = controller.analyze(false);
  await tick();
  await controller.load();
  assert.equal(reads, 0);
  assert.equal(state.error, "cancel unavailable");
  await controller.load();
  answer.reject(new Error("discarded old error"));
  await analysis;
  assert.equal(reads, 1);
  assert.equal(cancellations, 2);
  assert.equal(state.report.snapshot.fingerprint, "recovered");
  assert.equal(state.error, "");
});

test("stale snapshots and server blockers cannot invoke period AI", async () => {
  const state = {
    ...ui.initialPeriodReportState(),
    report: report("old", { stale: true }),
  };
  const controller = ui.createPeriodReportController(
    {
      getReport: async () => state.report,
      analyzePeriod: () => assert.fail("blocked AI must not reach the port"),
      cancelPeriod: async () => {},
    },
    state,
  );
  await controller.analyze(false);
  state.report = report("source", {
    aiDisabledReason: "Нужны 3 диктовки и 100 слов.",
  });
  await controller.analyze(false);
  assert.equal(state.analyzing, false);
});

test("AI errors check current freshness without losing the provider error", async () => {
  const changed = report("previous-snapshot", { stale: true });
  const state = { ...ui.initialPeriodReportState(), report: report("source") };
  let reads = 0;
  const controller = ui.createPeriodReportController(
    {
      getReport: async (days, refresh) => {
        assert.equal(days, 7);
        assert.equal(refresh, false);
        reads++;
        return changed;
      },
      analyzePeriod: async () => {
        throw new Error("source changed at midnight");
      },
      cancelPeriod: async () => {},
    },
    state,
  );
  await controller.analyze(false);
  assert.equal(reads, 1);
  assert.equal(state.report, changed);
  assert.equal(state.error, "source changed at midnight");
  assert.equal(state.analyzing, false);
});

test("a recovery read failure does not replace the original AI failure", async () => {
  const base = report("source");
  const state = { ...ui.initialPeriodReportState(), report: base };
  const controller = ui.createPeriodReportController(
    {
      getReport: async () => {
        throw new Error("secondary storage error");
      },
      analyzePeriod: async () => {
        throw new Error("original provider error");
      },
      cancelPeriod: async () => {},
    },
    state,
  );
  await controller.analyze(false);
  assert.equal(state.report, base);
  assert.equal(state.error, "original provider error");
});

test("successful explicit text AI keeps consent checked and blocks the same scope", async () => {
  for (const scope of ["findings", "original_text"]) {
    const configuration = {
      enabled: true,
      profileId: "cloud-profile",
      profileName: "Cloud profile",
      model: "model",
      dataScope: scope,
      cloud: true,
      cloudConsent: true,
    };
    const current = ref(
      report("source", {
        aiFingerprint: "numeric-request",
        aiConfiguration: configuration,
      }),
    );
    const shareText = ref(true);
    const stop = watch(
      () => ui.periodConsentKey(current.value, "same-connection"),
      () => {
        shareText.value = false;
      },
    );
    try {
      current.value = report("source", {
        aiFingerprint: "saved-text-request",
        aiConfiguration: configuration,
        aiStale: false,
        aiSummary: {
          dataScope: scope,
          generatedAt: "2026-10-12T12:00:00Z",
          summary: "New AI response",
        },
      });
      await nextTick();
      assert.equal(shareText.value, true);
      assert.match(
        ui.periodAiDisabledReason(current.value, shareText.value),
        /уже разобран/,
      );
    } finally {
      stop();
    }
  }
});

test("source, same-profile URL and configuration changes require new text consent", async () => {
  const configuration = {
    enabled: true,
    profileId: "same-profile",
    profileName: "Profile",
    model: "model",
    dataScope: "findings",
    cloud: true,
    cloudConsent: true,
  };
  const base = report("source", { aiConfiguration: configuration });
  const key = ui.periodConsentKey(base, "https://first.example/v1");
  assert.notEqual(ui.periodConsentKey(base, "https://second.example/v1"), key);
  assert.notEqual(
    ui.periodConsentKey(
      report("new-source", { aiConfiguration: configuration }),
      "https://first.example/v1",
    ),
    key,
  );
  for (const [field, value] of [
    ["profileId", "another-profile"],
    ["profileName", "Renamed profile"],
    ["model", "another-model"],
    ["dataScope", "original_text"],
    ["cloud", false],
    ["cloudConsent", false],
    ["enabled", false],
  ]) {
    assert.notEqual(
      ui.periodConsentKey(
        report("source", {
          aiConfiguration: { ...configuration, [field]: value },
        }),
        "https://first.example/v1",
      ),
      key,
      field,
    );
  }

  const current = ref(base);
  const connection = ref("https://first.example/v1");
  const shareText = ref(true);
  const stop = watch(
    () => ui.periodConsentKey(current.value, connection.value),
    () => {
      shareText.value = false;
    },
  );
  try {
    connection.value = "https://second.example/v1";
    await nextTick();
    assert.equal(shareText.value, false);
    shareText.value = true;
    current.value = report("new-source", { aiConfiguration: configuration });
    await nextTick();
    assert.equal(shareText.value, false);
    shareText.value = true;
    current.value = report("new-source", {
      aiConfiguration: { ...configuration, dataScope: "original_text" },
    });
    await nextTick();
    assert.equal(shareText.value, false);
  } finally {
    stop();
  }
});
