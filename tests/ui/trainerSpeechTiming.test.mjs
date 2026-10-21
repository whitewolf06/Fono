import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";
let server, period, timing, chart, defaults, createMockTrainer, mapping;
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
  timing = await server.ssrLoadModule(
    "/src/v3/features/trainer/domain/speechTiming.ts",
  );
  chart = await server.ssrLoadModule(
    "/src/v3/features/trainer/application/chart.ts",
  );
  ({ defaults } = await server.ssrLoadModule(
    "/src/v3/features/preferences/domain/preferences.ts",
  ));
  ({ createMockTrainer } = await server.ssrLoadModule(
    "/src/v3/features/trainer/infrastructure/mockTrainer.ts",
  ));
  mapping = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/historyMetadata.ts",
  );
});
after(async () => server?.close());
const now = Date.parse("2026-10-21T12:00:00Z");
const preferences = () => ({
  ...structuredClone(defaults),
  historyEnabled: true,
  analyticsConsent: true,
  trainerEnabled: true,
  retentionDays: 90,
});
const summary = (
  speechTimeMs,
  pauseTimeMs = 0,
  pauseCount = 0,
  maxPauseMs = 0,
) => ({ schemaVersion: 1, speechTimeMs, pauseTimeMs, pauseCount, maxPauseMs });
const entry = (id, speechTiming, original = "один два три") => ({
  id: String(id),
  createdAt: new Date(now).toISOString(),
  text: "result",
  title: "fixture",
  original,
  duration: 1000,
  metadata: { speechTiming },
});
test("tempo uses the words of measured sessions and sums of speech time, not mean session rates", () => {
  const report = period.buildTrainerSnapshot(
    [
      entry(1, summary(2000, 200, 1, 200)),
      entry(2, summary(8000, 2800, 3, 2000), "один два три четыре пять шесть"),
      entry(3, undefined, "много старых слов без измерения времени речи"),
    ],
    preferences(),
    30,
    now,
  );
  const m = timing.speechTimingMetrics(report.speechTiming);
  assert.equal(report.speechTiming.measuredSessions, 2);
  assert.equal(report.speechTiming.unknownSessions, 1);
  assert.equal(m.wordsPerMinute, 54);
  assert.equal(m.averagePauseMs, 750);
  assert.equal(m.maxPauseMs, 2000);
  assert.ok(Math.abs(m.pausePercent - (3000 / 13000) * 100) < 1e-10);
});
test("30 and 90 days include all 540 sessions beyond the previous cap", () => {
  for (const days of [30, 90]) {
    const entries = Array.from({ length: 540 }, (_, i) => ({
      ...entry(i, i % 2 ? undefined : summary(2000, 1000, 2, 750)),
      createdAt: new Date(now - (i % (days * 24 - 12)) * 3600000).toISOString(),
    }));
    const report = period.buildTrainerSnapshot(
      entries,
      preferences(),
      days,
      now,
    );
    assert.equal(report.includedSessions, 540);
    assert.equal(report.speechTiming.measuredSessions, 270);
    assert.equal(report.speechTiming.unknownSessions, 270);
    assert.equal(
      timing.speechTimingMetrics(report.speechTiming).wordsPerMinute,
      90,
    );
    assert.equal(
      report.daily.reduce((n, d) => n + d.speechTiming.measuredSessions, 0),
      270,
    );
  }
});
test("legacy, malformed and missing measurements remain unknown while measured zero pauses remain zero", () => {
  const report = period.buildTrainerSnapshot(
    [entry(1, summary(2000)), entry(2), entry(3, summary(0, 1000, 1, 1000))],
    preferences(),
    7,
    now,
  );
  assert.equal(report.speechTiming.measuredSessions, 1);
  assert.equal(report.speechTiming.unknownSessions, 2);
  assert.equal(timing.speechTimingMetrics(report.speechTiming).pausePercent, 0);
  assert.equal(timing.speechTimingMetrics(undefined).wordsPerMinute, null);
  const columns = chart.dailyChartColumns(report.daily, "tempo");
  assert.equal(columns.filter((c) => c.known).length, 1);
  assert.equal(columns.find((c) => c.known).total, 90);
  assert.equal(
    chart.dailyChartColumns(report.daily, "pauses").find((c) => c.known).total,
    0,
  );
});
test("VAD changes invalidate report identity and stale/opt-out caches erase timing for every day", async () => {
  const state = {
    preferences: preferences(),
    profiles: [],
    history: [entry(1, summary(2000))],
  };
  const trainer = createMockTrainer(
    state,
    () => now,
    async () => {},
  );
  const first = await trainer.port.getReport(30);
  state.history[0].metadata.speechTiming = summary(3000);
  const stale = await trainer.port.getReport(30);
  assert.equal(stale.stale, true);
  assert.notEqual(stale.currentFingerprint, first.currentFingerprint);
  assert.equal(stale.snapshot.speechTiming, null);
  assert.ok(stale.snapshot.daily.every((d) => d.speechTiming === null));
  state.preferences.analyticsConsent = false;
  await trainer.port.getReport(30);
  assert.equal(state.history[0].metadata.speechTiming, undefined);
});
test("native metadata maps only consented session timing, without a duration fallback", () => {
  const raw = {
    analytics_included: true,
    original_text: "text",
    metadata: {
      speech_timing: {
        schema_version: 1,
        speech_time_ms: 2000,
        pause_count: 0,
        pause_time_ms: 0,
        max_pause_ms: 0,
      },
    },
  };
  assert.deepEqual(
    mapping.historyMetadataFromNative(raw).speechTiming,
    summary(2000),
  );
  assert.equal(
    mapping.historyMetadataFromNative({ ...raw, analytics_included: false })
      .speechTiming,
    undefined,
  );
  assert.equal(
    mapping.historyMetadataFromNative({
      ...raw,
      metadata: { recording_duration_ms: 2000 },
    }).speechTiming,
    undefined,
  );
});
