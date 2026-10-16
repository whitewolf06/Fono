import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";

let server, api, errors, invoke;
const callbacks = new Map(),
  listeners = new Map();
before(async () => {
  globalThis.window = {
    __TAURI_INTERNALS__: {
      invoke: (...args) => invoke(...args),
      transformCallback(callback) {
        const id = callbacks.size + 1;
        callbacks.set(id, callback);
        return id;
      },
    },
    __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} },
  };
  server = await createServer({
    appType: "custom",
    configFile: "vite.config.ts",
    server: { hmr: false, middlewareMode: true },
  });
  api = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/ipc.ts",
  );
  errors = await server.ssrLoadModule(
    "/src/v3/shared/infrastructure/native/nativeErrors.ts",
  );
});
after(async () => {
  await server?.close();
  delete globalThis.window;
});

test("invoke maps stable model error codes and rejects untrusted details and parameters", async () => {
  invoke = async () => {
    throw {
      code: "model_missing",
      params: { detail: "PRIVATE_KEY C:\\profile\\model", path: "PRIVATE_KEY" },
    };
  };
  await assert.rejects(api.call("start_dictation"), (error) => {
    assert.equal(error.code, "model_missing");
    assert.match(error.message, /Файл выбранной модели не найден/);
    assert.deepEqual(error.params, {});
    assert.ok(!JSON.stringify(error).includes("PRIVATE_KEY"));
    return true;
  });
  invoke = async () => {
    throw { code: "backend_unavailable", params: { backend: "cuda" } };
  };
  await assert.rejects(
    api.call("start_dictation"),
    /Ускорение «CUDA» недоступно/,
  );
});

test("legacy Russian rejections keep authored actions while old English model errors become Russian", () => {
  assert.equal(
    errors.nativeErrorMessage(
      "ошибка конфигурации: Выберите подключение для обработки",
    ),
    "Выберите подключение для обработки",
  );
  assert.equal(
    errors.nativeErrorMessage(new Error("Буфер недоступен")),
    "Буфер недоступен",
  );
  assert.match(
    errors.nativeErrorMessage(
      "ошибка whisper/stt: model file not found: C:\\PRIVATE_PROFILE\\model.bin",
    ),
    /Файл выбранной модели не найден/,
  );
  assert.match(
    errors.nativeErrorMessage(
      "Whisper model is not selected. Download and choose a model in settings.",
    ),
    /Выберите модель распознавания/,
  );
  assert.match(
    errors.nativeErrorMessage(
      JSON.stringify({ code: "model_not_loaded", params: {} }),
    ),
    /Модель распознавания ещё не загружена/,
  );
});

test("unknown errors and arbitrary Cyrillic text cannot leak sources, secrets or huge payloads", () => {
  const cyclic = {};
  cyclic.self = cyclic;
  for (const input of [
    "Включите обработку текста PRIVATE_KEY",
    "Мой личный ключ PRIVATE_KEY",
    new Error("PRIVATE_KEY /home/profile/private"),
    { code: "unknown_future_error", message: "PRIVATE_KEY", details: cyclic },
    { code: "io", params: { detail: "Я".repeat(100_000) + "PRIVATE_KEY" } },
    { code: "backend_unavailable", params: { backend: "PRIVATE_KEY" } },
    cyclic,
  ]) {
    const result = new errors.NativeIpcError(input);
    assert.match(result.message, /[А-Яа-я]/);
    assert.ok(result.message.length < 400);
    assert.ok(!JSON.stringify(result).includes("PRIVATE_KEY"));
    assert.ok(!result.message.includes("/home/"));
  }
});

test("successful status snapshots sanitize embedded errors without modifying session text or HTTP job shape", async () => {
  const pending = {
    sessionId: 24,
    originalText: "PRIVATE_TRANSCRIPT",
    error: "model file not found: C:\\PRIVATE_PROFILE\\model.bin",
  };
  invoke = async () => pending;
  const result = await api.call("get_pending_dictation");
  assert.equal(result.sessionId, 24);
  assert.equal(result.originalText, "PRIVATE_TRANSCRIPT");
  assert.match(result.error, /Файл выбранной модели не найден/);
  assert.ok(
    pending.error.includes("PRIVATE_PROFILE"),
    "normalization does not mutate the native snapshot",
  );
  invoke = async () => ({
    error: "PRIVATE_KEY",
    history: {
      jobs: [
        {
          id: "job-1",
          error: { code: "model_not_ready", message: "PRIVATE_KEY" },
          result: { text: "PRIVATE_TRANSCRIPT" },
        },
      ],
    },
    queue: { jobs: [] },
  });
  const service = await api.call("get_local_transcription_service_snapshot");
  const job = service.history.jobs[0];
  assert.equal(job.id, "job-1");
  assert.equal(job.error.code, "model_not_ready");
  assert.equal(job.result.text, "PRIVATE_TRANSCRIPT");
  assert.ok(!service.error.includes("PRIVATE_KEY"));
  assert.ok(!job.error.message.includes("PRIVATE_KEY"));
});

test("legacy and versioned event subscriptions share safe mapping and retain operation identity", async () => {
  invoke = async (command, args) => {
    if (command === "plugin:event|listen") {
      listeners.set(args.event, callbacks.get(args.handler));
      return args.handler;
    }
  };
  const received = [];
  await api.subscribe("error", (value) => received.push(value));
  listeners.get("error")({
    payload: "ошибка whisper/stt: PRIVATE_KEY /home/profile/model",
  });
  assert.match(received[0], /Не удалось распознать речь/);
  await api.subscribe("backend-event-v1", (value) => received.push(value));
  listeners.get("backend-event-v1")({
    payload: {
      schema_version: 1,
      kind: "error",
      payload: { code: "injection", message: "PRIVATE_KEY", operation_id: 42 },
    },
  });
  assert.equal(received[1].payload.operation_id, 42);
  assert.equal(received[1].schema_version, 1);
  assert.match(received[1].payload.message, /Скопируйте результат из Fono/);
  assert.ok(!JSON.stringify(received).includes("PRIVATE_KEY"));
});
