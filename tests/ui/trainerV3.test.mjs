import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";

let server, analysis, overview;
before(async () => {
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { hmr: false, middlewareMode: true },
  });
  analysis = await server.ssrLoadModule(
    "/src/v3/features/trainer/domain/analysis.ts",
  );
  overview = await server.ssrLoadModule(
    "/src/v3/features/trainer/application/overview.ts",
  );
});
after(async () => server?.close());

const entry = (id, overrides = {}) => ({
  id,
  title: id,
  createdAt: "2026-10-04T12:30:15.000Z",
  duration: 0,
  text: "Готовый текст",
  original: "Исходный текст диктовки",
  ...overrides,
});

test("trainer defaults to newest first independent of archive order and breaks metric ties by date", () => {
  const older = entry("older", {
    createdAt: "2026-10-03T12:30:15.000Z",
    duration: 10,
  });
  const newest = entry("newest", { duration: 10 });
  const archive = [older, newest];
  assert.deepEqual(
    overview.sortTrainerEntries(archive, "date").map((item) => item.id),
    ["newest", "older"],
  );
  assert.deepEqual(
    overview.sortTrainerEntries(archive, "duration").map((item) => item.id),
    ["newest", "older"],
  );
  assert.deepEqual(
    archive.map((item) => item.id),
    ["older", "newest"],
  );
});

test("duration sorting uses measured session duration and puts unknown durations last", () => {
  const entries = [
    entry("unknown"),
    entry("legacy", { duration: 65 }),
    entry("measured", {
      duration: 999,
      metadata: { recordingDurationMs: 42000 },
    }),
    entry("zero", { metadata: { recordingDurationMs: 0 } }),
  ];
  assert.deepEqual(
    overview.sortTrainerEntries(entries, "duration").map((item) => item.id),
    ["legacy", "measured", "zero", "unknown"],
  );
  assert.equal(
    overview.recordingDurationLabel(entries[0]),
    "Длительность не сохранена",
  );
  assert.equal(overview.recordingDurationLabel(entries[1]), "1 мин 05 с");
  assert.equal(overview.recordingDurationLabel(entries[2]), "42 с");
  assert.equal(
    overview.recordingDuration(
      entry("invalid", {
        duration: 10,
        metadata: { recordingDurationMs: Number.NaN },
      }),
    ),
    undefined,
  );
});

test("long-text sort compares words in the original transcript rather than the improved output", () => {
  const long = entry("long", {
    original: "Первая мысль и вторая мысль",
    text: "Итог",
  });
  const short = entry("short", {
    original: "Привет",
    text: "Очень длинный результат после обработки текста",
  });
  assert.deepEqual(
    overview.sortTrainerEntries([short, long], "length").map((item) => item.id),
    ["long", "short"],
  );
  assert.equal(
    overview.wordCount(
      entry("unicode", { original: "Ёлка, déjà vu; Fono 3." }),
    ),
    4,
  );
});

test("browser trainer detects contextual markers in the same categories as native analysis", () => {
  const findings = analysis.analyze(
    entry("markers", {
      original: "Ну, как бы, потом потом потом, то есть точнее...",
    }),
  );
  assert.deepEqual(
    findings.map((item) => [item.title, item.count]),
    [
      ["Слова-паразиты", 2],
      ["Повторы", 2],
      ["Самоисправления", 2],
      ["Незавершённые фразы", 1],
    ],
  );
  assert.equal(
    analysis
      .analyze(entry("clean", { original: "Моя мысль выражена понятно." }))
      .reduce((sum, finding) => sum + finding.count, 0),
    0,
  );
});

test("persisted native findings remain authoritative including categories with no detected markers", () => {
  const nativeFindings = [
    { title: "Самоисправления", count: 8, example: "то есть", advice: "Пауза" },
    { title: "Незавершённые фразы", count: 0, example: "", advice: "" },
  ];
  const native = entry("native", {
    original: "Текст без ожидаемых маркеров",
    findings: nativeFindings,
  });
  assert.equal(analysis.analyze(native), nativeFindings);
  assert.deepEqual(overview.aggregateFindings([native]), [
    { title: "Самоисправления", count: 8 },
    { title: "Незавершённые фразы", count: 0 },
  ]);
});

test("chart normalization compares different transcript lengths and never clips larger observations", () => {
  const short = entry("short", { original: "Два слова" });
  const long = entry("long", { original: "У этого текста всего четыре слова" });
  assert.equal(overview.chartValue(2, short, "density"), 100);
  assert.equal(overview.chartValue(2, long, "density"), 100 / 3);
  assert.equal(overview.chartValue(9, short, "count"), 9);
  assert.equal(
    overview.chartValue(9, entry("empty", { original: "" }), "density"),
    0,
  );
  const scale = overview.chartScale([0, 4, 27, 80]);
  assert.ok(scale.max >= 80);
  assert.ok(scale.ticks.includes(0));
  assert.equal(scale.ticks.at(-1), scale.max);
  assert.ok(overview.chartScale([0]).max > 0);
});

test("chart shows the most recent observations in chronological order even when the list is sorted otherwise", () => {
  const entries = Array.from({ length: 30 }, (_, index) =>
    entry(String(index), {
      createdAt: new Date(Date.UTC(2026, 9, 1, index)).toISOString(),
    }),
  );
  const visible = overview.chartEntries(entries.reverse());
  assert.equal(visible.length, 24);
  assert.equal(visible[0].id, "6");
  assert.equal(visible.at(-1).id, "29");
});

test("date label includes calendar date, time with seconds and a clear unknown value", () => {
  const value = overview.exactDateLabel("2026-10-04T12:30:15.000Z");
  assert.match(value, /04\.10\.2026/);
  assert.match(value, /\d{2}:30:15/);
  assert.equal(overview.exactDateLabel("not-a-date"), "Дата не сохранена");
});
