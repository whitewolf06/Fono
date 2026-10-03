import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";

let server, createMockDiagnosticReport, useDiagnosticReport;
before(async () => {
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { hmr: false, middlewareMode: true },
  });
  ({ createMockDiagnosticReport } = await server.ssrLoadModule(
    "/src/v3/features/diagnostic-report/infrastructure/mockReport.ts",
  ));
  ({ useDiagnosticReport } = await server.ssrLoadModule(
    "/src/v3/features/diagnostic-report/application/useDiagnosticReport.ts",
  ));
});
after(async () => await server?.close());

test("report excludes private text, settings, device names, errors and job results", async () => {
  const privateText =
    "PRIVATE_TRANSCRIPT sk-PRIVATE-KEY C:\\Users\\PRIVATE_USER";
  const state = {
    preferences: {
      wakeEnabled: true,
      processingEnabled: true,
      historyEnabled: true,
      trainerEnabled: false,
      serviceEnabled: true,
      microphone: privateText,
      model: privateText,
      wakePhrase: privateText,
      instruction: privateText,
      dictionaryEntries: [privateText],
      apiKey: privateText,
    },
    microphoneAvailable: true,
    aiAvailable: false,
    error: privateText,
    logs: [privateText],
    last: { draft: privateText },
    history: [{ text: privateText }],
    profiles: [{ apiKey: privateText, url: privateText }],
    devices: [{ label: privateText }],
    jobs: [
      { state: "queued", text: privateText },
      { state: "error", error: privateText },
    ],
  };
  const report = await createMockDiagnosticReport(state).collect();
  assert.equal(report.runtime, "mock");
  assert.equal(report.schemaVersion, 1);
  assert.ok(!report.text.includes("PRIVATE"));
  assert.match(report.text, /ожидают 1; выполняются 0; ошибки 1/);
  assert.match(report.text, /browser mock/);
  assert.match(report.text, /не измеряются/);
});

test("failed clipboard retains freshly collected report for manual copy", async () => {
  let collected = 0;
  const { preview, createAndCopy } = useDiagnosticReport(
    {
      collect: async () => ({
        schemaVersion: 1,
        runtime: "native",
        text: `SAFE REPORT ${++collected}`,
      }),
    },
    async () => {
      throw new Error("Clipboard unavailable");
    },
  );
  await assert.rejects(createAndCopy(), /Clipboard unavailable/);
  assert.equal(preview.value, "SAFE REPORT 1");
  await assert.rejects(createAndCopy(), /Clipboard unavailable/);
  assert.equal(preview.value, "SAFE REPORT 2");
});

test("one click collects and copies exact current report without side effects", async () => {
  const copied = [];
  const { preview, createAndCopy } = useDiagnosticReport(
    {
      collect: async () => ({
        schemaVersion: 1,
        runtime: "mock",
        text: "SAFE REPORT",
      }),
    },
    async (text) => copied.push(text),
  );
  assert.equal(preview.value, "");
  await createAndCopy();
  assert.deepEqual(copied, ["SAFE REPORT"]);
  assert.equal(preview.value, "SAFE REPORT");
});
