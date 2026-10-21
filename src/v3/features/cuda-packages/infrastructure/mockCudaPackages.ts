import { message as uiMessage } from "../../../shared/domain/messages";
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
    message: uiMessage("ui.b64385532b"),
    nvidiaDetected: null,
  });
  let generation = 0;
  let disposed = false;
  let active = false;
  return {
    state,
    refresh: async () => {},
    async install() {
      if (active || disposed) throw new Error(uiMessage("ui.bd5716d751"));
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
        state.message = uiMessage("ui.05542d0fad");
      } finally {
        active = false;
      }
    },
    async cancel() {
      if (!cudaPackageBusy(state.phase)) return;
      generation++;
      state.phase = "cancelled";
      state.message = uiMessage("ui.055483c6b8");
    },
    dispose() {
      disposed = true;
      generation++;
    },
  };
}
