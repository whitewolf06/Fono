import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";

let server, createNativeFiles, createMockFiles, jobFromNative;
before(async () => {
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { hmr: false, middlewareMode: true },
  });
  ({ createNativeFiles } = await server.ssrLoadModule(
    "/src/v3/features/files/infrastructure/nativeFiles.ts",
  ));
  ({ createMockFiles } = await server.ssrLoadModule(
    "/src/v3/features/files/infrastructure/mockFiles.ts",
  ));
  ({ jobFromNative } = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/mapping.ts",
  ));
});
after(async () => server?.close());

test("native files reuse job IDs, cancellation and explicit native export without renderer PCM", async () => {
  const calls = [];
  let reads = 0;
  const files = createNativeFiles(
    async (command, args) => {
      calls.push({ command, args });
      return command === "choose_transcription_files"
        ? { accepted: ["tr_file"], rejected: 2 }
        : true;
    },
    async () => {
      reads++;
    },
  );
  assert.deepEqual(await files.choose(), {
    accepted: ["tr_file"],
    rejected: 2,
  });
  await files.cancel("tr_file");
  assert.equal(await files.export("tr_file", "srt"), true);
  assert.equal(reads, 2);
  assert.deepEqual(calls, [
    { command: "choose_transcription_files", args: undefined },
    { command: "cancel_local_transcription_job", args: { id: "tr_file" } },
    {
      command: "export_transcription_file",
      args: { id: "tr_file", format: "srt" },
    },
  ]);
});

test("file identity, progress and actual timing survive the native mapping; old jobs stay supported", () => {
  const old = {
    id: "legacy",
    state: "completed",
    requested_language: "ru",
    result: { text: "старый", audio_seconds: 10 },
    error: null,
  };
  assert.equal(jobFromNative(old).file, false);
  assert.equal(jobFromNative(old).hasTimestamps, false);
  const file = jobFromNative({
    ...old,
    file_name: "Встреча.wav",
    progress: 0.54,
    state: "transcribing",
    result: {
      ...old.result,
      segments: [{ start_ms: 1000, end_ms: 2000, text: "слово" }],
    },
  });
  assert.equal(file.file, true);
  assert.equal(file.name, "Встреча.wav");
  assert.equal(file.progress, 0.54);
  assert.equal(file.state, "running");
  assert.equal(file.hasTimestamps, true);
  assert.equal(jobFromNative({ ...old, state: "preparing" }).preparing, true);
});

test("browser preview adds demo files with API disabled and never reads dropped audio", async () => {
  const state = { jobs: [], preferences: { serviceEnabled: false } };
  const files = createMockFiles(state);
  assert.equal((await files.choose()).accepted.length, 2);
  assert.equal(state.preferences.serviceEnabled, false);
  const listeners = new Map();
  globalThis.window = {
    addEventListener: (name, listener) => listeners.set(name, listener),
    removeEventListener: (name) => listeners.delete(name),
  };
  try {
    let added;
    const release = await files.bindDrop({
      hover() {},
      added: (value) => {
        added = value;
      },
      error: (error) => {
        throw error;
      },
    });
    listeners.get("drop")({
      preventDefault() {},
      dataTransfer: {
        files: [
          {
            name: "Личный файл.wav",
            arrayBuffer() {
              throw new Error("private audio must not be read");
            },
          },
          { name: "video.mp4" },
        ],
      },
    });
    assert.equal(added.accepted.length, 1);
    assert.equal(added.rejected, 1);
    assert.equal(state.jobs[0].name, "Личный файл.wav");
    release();
    assert.equal(listeners.size, 0);
  } finally {
    delete globalThis.window;
  }
});

test("file compatibility errors localize closed native guidance and keep results unchanged", async () => {
  const { bindMessageLocale, uiLocale } = await server.ssrLoadModule(
    "/src/v3/shared/domain/locale.ts",
  );
  const previous = uiLocale();
  let language = "en";
  bindMessageLocale(() => language);
  const job = {
    id: "file-compatibility",
    state: "failed",
    requested_language: "ru",
    file_name: "fixture.wav",
    result: { text: "PRIVATE_TRANSCRIPT", audio_seconds: 10 },
    error: {
      code: "invalid_request",
      message:
        "Сервер не поддерживает нужные возможности распознавания. Обновите Fono Server.",
    },
  };
  try {
    const english = jobFromNative(job);
    assert.equal(
      english.error,
      "The server does not support the required recognition capabilities. Update Fono Server.",
    );
    assert.equal(english.text, "PRIVATE_TRANSCRIPT");
    language = "ru";
    const russian = jobFromNative({
      ...job,
      error: { ...job.error, message: english.error },
    });
    assert.equal(russian.error, job.error.message);
    assert.equal(russian.text, job.result.text);
    assert.equal(jobFromNative({ ...job, error: null }).error, undefined);
  } finally {
    bindMessageLocale(() => previous);
  }
});
