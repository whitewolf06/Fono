import { reactive } from "vue";
import type {
  CudaPackagesPort,
  CudaPackageStatus,
} from "../../../shared/domain/cudaPackages";
import { cudaPackageBusy } from "../../../shared/domain/cudaPackages";

export function createMockCudaPackages(): CudaPackagesPort {
  const state = reactive<CudaPackageStatus>({
    phase: "not_installed",
    operationId: null,
    downloadedBytes: 0,
    totalBytes: null,
    installedRevision: null,
    message: "Демонстрация: пакет CUDA ещё не установлен.",
    nvidiaDetected: null,
  });
  let generation = 0;
  let disposed = false;
  let active = false;
  return {
    state,
    refresh: async () => {},
    async install() {
      if (active || disposed)
        throw new Error("Подготовка CUDA уже выполняется.");
      active = true;
      const ticket = ++generation;
      state.operationId = `demo-${ticket}`;
      state.phase = "downloading";
      state.totalBytes = 100;
      state.downloadedBytes = 0;
      try {
        for (let step = 1; step <= 4; step++) {
          await new Promise((resolve) => setTimeout(resolve, 150));
          if (disposed || generation !== ticket) return;
          state.downloadedBytes = step * 25;
        }
        state.phase = "installed";
        state.installedRevision = 1;
        state.message =
          "Демонстрация завершена. Файлы на компьютер не загружались.";
      } finally {
        active = false;
      }
    },
    async cancel() {
      if (!cudaPackageBusy(state.phase)) return;
      generation++;
      state.phase = "cancelled";
      state.message = "Демонстрационная загрузка отменена.";
    },
    dispose() {
      disposed = true;
      generation++;
    },
  };
}
