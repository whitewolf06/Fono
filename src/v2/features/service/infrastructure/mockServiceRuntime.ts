import type { ServiceRuntime } from "../application/serviceRuntime";
import type { ServiceSnapshot } from "../domain/serviceMonitor";

const mockSnapshot: ServiceSnapshot = {
  address: "127.0.0.1:17832",
  protocolVersion: 1,
  model: "Whisper Large v3 Turbo",
  acceleration: "CUDA",
  queue: {
    capacity: 4,
    queued: 1,
    preparing: 0,
    transcribing: 1,
    completed: 3,
    failed: 0,
    cancelled: 0,
    jobs: [
      {
        id: "tr_0000000000000005",
        state: "transcribing",
        createdAtMs: Date.now() - 14_000,
        startedAtMs: Date.now() - 9_000,
        finishedAtMs: null,
        result: null,
        error: null,
      },
      {
        id: "tr_0000000000000004",
        state: "queued",
        createdAtMs: Date.now() - 6_000,
        startedAtMs: null,
        finishedAtMs: null,
        result: null,
        error: null,
      },
      {
        id: "tr_0000000000000003",
        state: "completed",
        createdAtMs: Date.now() - 90_000,
        startedAtMs: Date.now() - 85_000,
        finishedAtMs: Date.now() - 78_000,
        result: {
          text: "Проверяю локальный сервис распознавания и очередь задач.",
          detectedLanguage: "ru",
          audioSeconds: 12.4,
          transcribeSeconds: 0.7,
          model: "large_turbo",
          backend: "CUDA",
        },
        error: null,
      },
    ],
  },
};

export function createMockServiceRuntime(): ServiceRuntime {
  return {
    getSnapshot: async () => mockSnapshot,
    cancelJob: async () => undefined,
    copyText: async () => undefined,
  };
}
