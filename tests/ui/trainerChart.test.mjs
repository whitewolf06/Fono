import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer as createTcpServer } from "node:net";
import { createServer } from "vite";

let server, chart;
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
  chart = await server.ssrLoadModule(
    "/src/v3/features/trainer/application/chart.ts",
  );
});
after(async () => server?.close());

const day = (index, options = {}) => ({
  date: new Date(Date.UTC(2026, 0, index + 1)).toISOString().slice(0, 10),
  sessions: 0,
  words: 0,
  counts: { filler: 0, repetition: 0, selfCorrection: 0, unfinished: 0 },
  density: 0,
  ...options,
});

test("7/30/90 day charts retain each empty calendar day in order", () => {
  for (const length of [7, 30, 90]) {
    const days = Array.from({ length }, (_, index) => day(index));
    const before = structuredClone(days);
    const columns = chart.dailyChartColumns(days, "count");
    assert.equal(columns.length, length);
    assert.deepEqual(
      columns.map((column) => column.day.date),
      days.map((item) => item.date),
    );
    assert.ok(columns.every((column) => column.total === 0));
    assert.ok(
      columns.every(
        (column, index) =>
          index === 0 || column.offset > columns[index - 1].offset,
      ),
    );
    assert.deepEqual(days, before);
  }
});

test("readable axis ticks keep both period boundaries without duplicate labels", () => {
  for (const length of [7, 30, 90]) {
    const indices = chart.chartTickIndices(length, length === 7 ? 3 : 5);
    assert.equal(indices[0], 0);
    assert.equal(indices.at(-1), length - 1);
    assert.equal(new Set(indices).size, indices.length);
    assert.ok(indices.every((index) => index >= 0 && index < length));
    assert.ok(
      indices.every(
        (index, offset) => offset === 0 || index > indices[offset - 1],
      ),
    );
  }
  assert.deepEqual(chart.chartTickIndices(0), []);
  assert.deepEqual(chart.chartTickIndices(1), [0]);
  assert.deepEqual(chart.chartTickIndices(2), [0, 1]);
});

test("stacked counts and density use all four categories and actual daily words", () => {
  const source = day(0, {
    sessions: 2,
    words: 200,
    counts: { filler: 4, repetition: 2, selfCorrection: 1, unfinished: 1 },
    density: 4,
  });
  const count = chart.dailyChartColumns([source], "count")[0];
  const density = chart.dailyChartColumns([source], "density")[0];
  assert.equal(count.segments.length, 4);
  assert.deepEqual(
    count.segments.map((segment) => segment.value),
    [4, 2, 1, 1],
  );
  assert.deepEqual(
    count.segments.map((segment) => segment.start),
    [0, 4, 6, 7],
  );
  assert.equal(count.total, 8);
  assert.deepEqual(
    density.segments.map((segment) => segment.value),
    [2, 1, 0.5, 0.5],
  );
  assert.deepEqual(
    density.segments.map((segment) => segment.start),
    [0, 2, 3, 3.5],
  );
  assert.equal(density.total, source.density);
});

test("zero-word days have finite zero density without inventing a denominator", () => {
  const source = day(0, {
    counts: { filler: 1, repetition: 0, selfCorrection: 0, unfinished: 0 },
  });
  const column = chart.dailyChartColumns([source], "density")[0];
  assert.equal(column.total, 0);
  assert.ok(column.segments.every((segment) => Number.isFinite(segment.value)));
  assert.equal(chart.dailyChartColumns([source], "count")[0].total, 1);
});

test("initial selected day is the latest included day, including zero-marker sessions", () => {
  const days = [day(0, { sessions: 1 }), day(1, { sessions: 2 }), day(2)];
  assert.equal(chart.preferredDayIndex(days), 1);
  assert.equal(chart.preferredDayIndex([day(0), day(1)]), 1);
  assert.equal(chart.preferredDayIndex([]), -1);
});

test("column centers are evenly spaced and remain inside the plot", () => {
  for (const length of [1, 7, 30, 90]) {
    const offsets = Array.from({ length }, (_, index) =>
      chart.chartColumnOffset(index, length),
    );
    assert.ok(offsets.every((offset) => offset > 0 && offset < 100));
    assert.ok(Math.abs(offsets[0] + offsets.at(-1) - 100) < 0.000001);
  }
});
