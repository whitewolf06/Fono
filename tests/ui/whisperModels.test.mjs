import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer } from "vite";

let server;
let whisperModels;

before(async () => {
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { middlewareMode: true },
  });
  whisperModels = await server.ssrLoadModule(
    "/src/v2/shared/domain/whisperModels.ts",
  );
});

after(async () => {
  await server?.close();
});

test("large v3 model path round-trips through the settings label", () => {
  const size = whisperModels.whisperModelSizeFromPath(
    "C:/models/ggml-large-v3.bin",
  );

  assert.equal(size, "large");
  assert.equal(whisperModels.whisperModelSelectionLabel(size), "Whisper Large v3");
  assert.equal(
    whisperModels.whisperModelSizeFromSelection("Whisper Large v3"),
    "large",
  );
});

test("large v3 turbo is never coerced to small", () => {
  const size = whisperModels.whisperModelSizeFromPath(
    "C:\\Users\\tester\\Fono\\whisper-models\\ggml-large-v3-turbo.bin",
  );

  assert.equal(size, "large_turbo");
  assert.equal(
    whisperModels.whisperModelSelectionLabel(size),
    "Whisper Large v3 Turbo",
  );
  assert.equal(
    whisperModels.whisperModelSizeFromSelection("Whisper Large v3 Turbo"),
    "large_turbo",
  );
});
