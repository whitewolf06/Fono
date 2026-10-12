import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer } from "vite";

let server,
  period,
  createMockTrainer,
  createNativeTrainer,
  defaults,
  mapping,
  observeTrainerDay;
before(async () => {
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    optimizeDeps: { noDiscovery: true, include: [] },
    server: { hmr: false, middlewareMode: true },
  });
  period = await server.ssrLoadModule(
    "/src/v3/features/trainer/domain/period.ts",
  );
  ({ createMockTrainer } = await server.ssrLoadModule(
    "/src/v3/features/trainer/infrastructure/mockTrainer.ts",
  ));
  ({ createNativeTrainer } = await server.ssrLoadModule(
    "/src/v3/features/trainer/infrastructure/nativeTrainer.ts",
  ));
  ({ defaults } = await server.ssrLoadModule(
    "/src/v3/features/preferences/domain/preferences.ts",
  ));
  mapping = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/mapping.ts",
  );
  ({ observeTrainerDay } = await server.ssrLoadModule(
    "/src/v3/features/trainer/infrastructure/dayChange.ts",
  ));
});
after(async () => server?.close());
const now = Date.parse("2026-10-12T12:00:00Z");
const entry = (
  id,
  original = "Ну, как бы, потом потом. То есть точнее мысль...",
  overrides = {},
) => ({
  id,
  original,
  text: "Результат",
  title: id,
  duration: 10,
  createdAt: "2026-10-12T10:00:00Z",
  ...overrides,
});
const preferences = () => ({
  ...structuredClone(defaults),
  trainerEnabled: true,
  analyticsConsent: true,
  retentionDays: 90,
  trainerAiEnabled: true,
  trainerProfile: "local",
  trainerModel: "test",
  trainerScope: "metrics_only",
  cloudConsent: false,
});
const state = () => ({
  preferences: preferences(),
  aiAvailable: true,
  profiles: [
    {
      id: "local",
      name: "Локальная модель",
      model: "test",
      location: "local",
      url: "http://localhost:1234",
    },
  ],
  history: [1, 2, 3].map((id) =>
    entry(
      String(id),
      "Ну, основная мысль выражена понятно и завершается после короткой паузы. ".repeat(
        8,
      ),
    ),
  ),
});

test("period rules normalize case, respect word boundaries and keep explainable session origins", () => {
  const snapshot = period.buildTrainerSnapshot(
    [entry("a", "НУ, ну. Нуля. Как бы, КАК БЫ. Потом потом. ТО ЕСТЬ...")],
    preferences(),
    7,
    now,
  );
  assert.equal(snapshot.counts.filler, 4);
  assert.equal(snapshot.counts.repetition, 2);
  assert.equal(snapshot.counts.selfCorrection, 1);
  assert.equal(snapshot.counts.unfinished, 1);
  const filler = snapshot.terms.find((term) => term.key === "filler:ну");
  assert.equal(filler.count, 2);
  assert.equal(filler.affectedSessions, 1);
  assert.deepEqual(filler.sessionIds, ["a"]);
  assert.equal(filler.examples[0].fragment, "НУ");
  assert.equal(snapshot.daily.length, 7);
  assert.equal(snapshot.daily.filter((day) => day.sessions).length, 1);
});

test("period eligibility excludes opted-out, pending, expired and unavailable transcripts", () => {
  const snapshot = period.buildTrainerSnapshot(
    [
      entry("included"),
      entry("excluded", "PRIVATE_EXCLUDED", { analyticsIncluded: false }),
      entry("pending", "PRIVATE_PENDING", { analysisReady: false }),
      entry("missing", undefined, { original: undefined }),
      entry("expired", "PRIVATE_EXPIRED", {
        createdAt: "2026-10-10T08:00:00Z",
      }),
    ],
    { ...preferences(), retentionDays: 1 },
    7,
    now,
  );
  assert.equal(snapshot.includedSessions, 1);
  assert.equal(snapshot.excludedSessions, 1);
  assert.equal(snapshot.pendingSessions, 1);
  assert.equal(snapshot.unavailableSessions, 2);
  assert.deepEqual(snapshot.sessionIds, ["included"]);
  assert.doesNotMatch(JSON.stringify(snapshot), /PRIVATE_/);
});

test("range uses fixed UTC calendar boundaries, even for offset session timestamps", () => {
  const snapshot = period.buildTrainerSnapshot(
    [
      entry("outside", "ну", { createdAt: "2026-10-05T23:59:59Z" }),
      entry("inside", "ну", { createdAt: "2026-10-06T03:00:00+03:00" }),
      entry("future-day", "ну", { createdAt: "2026-10-13T00:00:00Z" }),
    ],
    preferences(),
    7,
    now,
  );
  assert.equal(snapshot.from, "2026-10-06T00:00:00.000Z");
  assert.equal(snapshot.to, "2026-10-13T00:00:00.000Z");
  assert.deepEqual(snapshot.sessionIds, ["inside"]);
  assert.equal(snapshot.daily[0].sessions, 1);
});

test("fingerprint is stable under archive ordering and processed text changes, but tracks source eligibility", () => {
  const source = [entry("a"), entry("b")];
  const fingerprint = period.trainerFingerprint(source, preferences(), 7, now);
  assert.equal(
    fingerprint,
    period.trainerFingerprint(
      [...source].reverse(),
      preferences(),
      7,
      now + 1000,
    ),
  );
  assert.equal(
    fingerprint,
    period.trainerFingerprint(
      source.map((item) => ({ ...item, text: "Изменённый результат" })),
      preferences(),
      7,
      now,
    ),
  );
  assert.notEqual(
    fingerprint,
    period.trainerFingerprint(
      source.map((item) => ({ ...item, analyticsIncluded: false })),
      preferences(),
      7,
      now,
    ),
  );
  assert.notEqual(
    fingerprint,
    period.trainerFingerprint(
      source,
      { ...preferences(), retentionDays: 1 },
      7,
      now,
    ),
  );
});

test("mock caches local snapshot, marks newly added sources stale, refreshes explicitly and never starts AI on read", async () => {
  const source = state();
  let aiCalls = 0;
  const trainer = createMockTrainer(
    source,
    () => now,
    async () => {
      aiCalls++;
    },
  );
  const first = await trainer.port.getReport(7);
  source.history.push(entry("new"));
  const stale = await trainer.port.getReport(7);
  assert.equal(stale.stale, true);
  assert.equal(stale.snapshot.fingerprint, first.snapshot.fingerprint);
  const refreshed = await trainer.port.getReport(7, true);
  assert.equal(refreshed.stale, false);
  assert.equal(refreshed.snapshot.includedSessions, 4);
  assert.equal(aiCalls, 0);
});

test("mock purges examples and AI after deletion or exclusion rather than returning private stale sources", async () => {
  const source = state();
  source.history[0].original += " СЕКРЕТНОЕ СЕКРЕТНОЕ";
  const trainer = createMockTrainer(
    source,
    () => now,
    async () => {},
  );
  const first = await trainer.port.getReport(7);
  await trainer.port.analyzePeriod({
    days: 7,
    fingerprint: first.snapshot.fingerprint,
    shareText: false,
  });
  source.history[0].analyticsIncluded = false;
  const excluded = await trainer.port.getReport(7);
  assert.equal(excluded.stale, true);
  assert.deepEqual(excluded.snapshot.terms, []);
  assert.equal(excluded.aiSummary, null);
  assert.doesNotMatch(JSON.stringify(excluded), /СЕКРЕТНОЕ/);
  assert.equal(
    (await trainer.port.getReport(7, true)).snapshot.includedSessions,
    2,
  );
});

test("same-fresh refresh preserves snapshot time and manual AI instead of enabling another request", async () => {
  const source = state();
  let time = now,
    requests = 0;
  const trainer = createMockTrainer(
    source,
    () => time,
    async () => {
      requests++;
    },
  );
  const first = await trainer.port.getReport(7);
  const input = {
    days: 7,
    fingerprint: first.snapshot.fingerprint,
    shareText: false,
  };
  const analyzed = await trainer.port.analyzePeriod(input);
  time += 3_600_000;
  const refreshed = await trainer.port.getReport(7, true);
  assert.equal(refreshed.snapshot.generatedAt, first.snapshot.generatedAt);
  assert.deepEqual(refreshed.aiSummary, analyzed.aiSummary);
  await trainer.port.analyzePeriod(input);
  assert.equal(requests, 1);
});

test("day adapter reports UTC rollover once and releases its scheduling resource", () => {
  let time = now,
    tick,
    stopped = false,
    calls = 0;
  const release = observeTrainerDay(
    () => {
      calls++;
    },
    () => time,
    (handler) => {
      tick = handler;
      return () => {
        stopped = true;
      };
    },
  );
  tick();
  assert.equal(calls, 0);
  time = Date.parse("2026-10-13T00:00:00Z");
  tick();
  tick();
  assert.equal(calls, 1);
  release();
  assert.equal(stopped, true);
});

test("manual AI caches exact parameters and records provenance; a changed profile invalidates it", async () => {
  const source = state();
  let aiCalls = 0;
  const trainer = createMockTrainer(
    source,
    () => now,
    async () => {
      aiCalls++;
    },
  );
  const report = await trainer.port.getReport(7);
  const input = {
    days: 7,
    fingerprint: report.snapshot.fingerprint,
    shareText: false,
  };
  const analyzed = await trainer.port.analyzePeriod(input);
  assert.equal(analyzed.aiSummary.dataScope, "metrics_only");
  assert.equal(analyzed.aiSummary.model, "test");
  assert.equal(analyzed.aiSummary.generatedAt, "2026-10-12T12:00:00.000Z");
  await trainer.port.analyzePeriod(input);
  assert.equal(aiCalls, 1);
  source.preferences.trainerModel = "new-model";
  assert.equal((await trainer.port.getReport(7)).aiStale, true);
  await trainer.port.analyzePeriod(input);
  assert.equal(aiCalls, 2);
});

test("cloud consent and small sample block manual AI without damaging local metrics", async () => {
  const source = state();
  source.profiles[0].location = "cloud";
  const trainer = createMockTrainer(
    source,
    () => now,
    async () => {
      throw new Error("must not send");
    },
  );
  const report = await trainer.port.getReport(7);
  await assert.rejects(
    trainer.port.analyzePeriod({
      days: 7,
      fingerprint: report.snapshot.fingerprint,
      shareText: false,
    }),
    /облако/,
  );
  assert.equal(report.snapshot.includedSessions, 3);
  source.history = [entry("small")];
  const small = await trainer.port.getReport(7, true);
  assert.match(small.aiDisabledReason, /3 диктовки и 100 слов/);
});

test("cancellation and changed source data reject late manual AI results", async () => {
  for (const change of ["cancel", "data", "settings"]) {
    const source = state();
    let release, started;
    const ready = new Promise((resolve) => {
      started = resolve;
    });
    const wait = new Promise((resolve) => {
      release = resolve;
    });
    const trainer = createMockTrainer(
      source,
      () => now,
      async () => {
        started();
        await wait;
      },
    );
    const report = await trainer.port.getReport(7);
    const operation = trainer.port.analyzePeriod({
      days: 7,
      fingerprint: report.snapshot.fingerprint,
      shareText: false,
    });
    await ready;
    if (change === "cancel") await trainer.port.cancelPeriod();
    if (change === "data") source.history.push(entry("late"));
    if (change === "settings") source.preferences.trainerModel = "changed";
    release();
    await assert.rejects(operation, /отменён|изменились/);
    assert.equal((await trainer.port.getReport(7)).aiSummary, null);
  }
});

test("native port sends explicit scope/request ID and cancellation remains tied to the running operation", async () => {
  const calls = [];
  let complete;
  const port = createNativeTrainer(
    async (command, args) => {
      calls.push([command, args]);
      if (command === "analyze_trainer_period")
        return await new Promise((resolve) => {
          complete = resolve;
        });
      return {};
    },
    async () => {},
    () => "operation-1",
  );
  const running = port.analyzePeriod({
    days: 30,
    fingerprint: "snapshot-1",
    shareText: false,
  });
  await port.cancelPeriod();
  assert.deepEqual(calls, [
    [
      "analyze_trainer_period",
      {
        days: 30,
        fingerprint: "snapshot-1",
        shareText: false,
        requestId: "operation-1",
      },
    ],
    ["cancel_trainer_period", { requestId: "operation-1" }],
  ]);
  await assert.rejects(
    port.analyzePeriod({ days: 7, fingerprint: "other", shareText: false }),
    /завершения/,
  );
  complete({});
  await running;
});

test("native history mapping preserves explicit exclusions and defaults legacy entries to included", () => {
  const raw = {
    id: "a",
    text: "Результат",
    created_at: "2026-10-12T10:00:00Z",
    analysis_status: "ready",
  };
  assert.equal(
    mapping.historyFromNative({ ...raw, analytics_included: false })
      .analyticsIncluded,
    false,
  );
  assert.equal(mapping.historyFromNative(raw).analyticsIncluded, true);
});
