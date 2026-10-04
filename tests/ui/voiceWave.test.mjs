import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer } from "vite";

let server, appearance;
before(async () => {
  server = await createServer({
    appType: "custom",
    configFile: false,
    server: { hmr: false, ws: false, middlewareMode: true },
    optimizeDeps: { noDiscovery: true, include: [] },
  });
  ({ voiceWaveAppearance: appearance } = await server.ssrLoadModule(
    "/src/v3/features/dictation/presentation/voiceWaveAppearance.ts",
  ));
});
after(async () => server?.close());

test("wave rejects invalid levels and clamps its amplitude inside the SVG", () => {
  const quiet = appearance("listening", 0);
  const loud = appearance("listening", 1);
  for (const level of [NaN, Infinity, -Infinity, -2]) {
    assert.deepEqual(appearance("listening", level), quiet);
  }
  assert.deepEqual(appearance("listening", 7), loud);
  for (const phase of ["listening", "silence"]) {
    for (const level of [0, 0.05, 0.4, 1]) {
      const value = appearance(phase, level);
      assert.ok(value.amplitude > 0 && value.amplitude <= 1);
      assert.ok(value.opacity > 0 && value.opacity <= 1);
    }
  }
});

test("quieter speech gets visible gain and voice controls amplitude and glow", () => {
  const silent = appearance("listening", 0);
  const quiet = appearance("listening", 0.04);
  const loud = appearance("listening", 0.64);
  assert.ok(quiet.energy > 0.04);
  assert.ok(quiet.amplitude - silent.amplitude > 0.15);
  assert.ok(loud.amplitude > quiet.amplitude);
  assert.ok(loud.glow > quiet.glow);
  assert.ok(loud.strokeWidth > quiet.strokeWidth);
  assert.deepEqual(appearance("silence", 0.64), loud);
});

test("inactive and processing waves ignore stale microphone values", () => {
  for (const phase of [
    "idle",
    "done",
    "cancelled",
    "error",
    "processing",
    "transcribing",
  ]) {
    assert.deepEqual(appearance(phase, 1), appearance(phase, 0));
    assert.equal(appearance(phase, 1).energy, 0);
  }
  assert.ok(
    appearance("processing", 0).amplitude > appearance("idle", 0).amplitude,
  );
  assert.deepEqual(appearance("processing", 0), appearance("transcribing", 0));
});
