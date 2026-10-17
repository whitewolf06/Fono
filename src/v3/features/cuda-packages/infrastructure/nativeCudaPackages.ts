import { reactive } from "vue";
import type {
  CudaPackagesPort,
  CudaPackagePhase,
  CudaPackageStatus,
} from "../../../shared/domain/cudaPackages";
import { cudaPackageBusy } from "../../../shared/domain/cudaPackages";
import { call } from "../../../shared/infrastructure/native/ipc";

interface NativeSnapshot {
  phase: CudaPackagePhase;
  operation_id: string | null;
  downloaded_bytes: number;
  total_bytes: number | null;
  installed_revision: number | null;
  message: string | null;
  nvidia_detected?: boolean | null;
}

export function createNativeCudaPackages(
  transport = call,
  installed: () => Promise<unknown> = async () => {},
  visible: () => boolean = () => true,
  newId: () => string = () => globalThis.crypto.randomUUID(),
): CudaPackagesPort {
  const state = reactive<CudaPackageStatus>({
    phase: "not_installed",
    operationId: null,
    downloadedBytes: 0,
    totalBytes: null,
    installedRevision: null,
    message: "Загрузка состояния CUDA…",
  });
  let disposed = false;
  let revision = 0;
  let readId = 0;
  let pendingRead:
    { revision: number; id: number; promise: Promise<void> } | undefined;
  let installing: string | null = null;
  function apply(value: NativeSnapshot) {
    Object.assign(state, {
      phase: value.phase,
      operationId: value.operation_id,
      downloadedBytes: value.downloaded_bytes,
      totalBytes: value.total_bytes,
      installedRevision: value.installed_revision,
      message: value.message ?? "",
      nvidiaDetected: value.nvidia_detected ?? null,
    });
  }
  const isInstalled = () => state.phase === "installed";
  function refresh(): Promise<void> {
    if (disposed || !visible()) return Promise.resolve();
    const ticket = revision;
    if (pendingRead?.revision === ticket) return pendingRead.promise;
    const id = ++readId;
    const promise = (async () => {
      const value = await transport<NativeSnapshot>("get_cuda_package_status");
      if (
        !disposed &&
        ticket === revision &&
        id === readId &&
        (!installing || value.operation_id === installing)
      )
        apply(value);
    })();
    pendingRead = { revision: ticket, id, promise };
    void promise
      .finally(() => {
        if (pendingRead?.id === id) pendingRead = undefined;
      })
      .catch(() => {});
    return promise;
  }
  const timer = setInterval(() => {
    if (cudaPackageBusy(state.phase)) void refresh().catch(() => {});
  }, 2000);
  const startup = revision;
  void refresh().catch(() => {
    if (disposed || startup !== revision) return;
    state.phase = "error";
    state.message = "Не удалось получить состояние CUDA. Повторите проверку.";
  });
  return {
    state,
    refresh,
    async install() {
      if (disposed || installing || cudaPackageBusy(state.phase))
        throw new Error("Подготовка CUDA уже выполняется.");
      if (state.phase === "unavailable")
        throw new Error("CUDA недоступна в этой сборке.");
      const operationId = newId();
      installing = operationId;
      const ticket = ++revision;
      state.phase = "checking";
      state.operationId = operationId;
      state.downloadedBytes = 0;
      state.totalBytes = null;
      state.message = "Проверяем подписанный пакет CUDA…";
      try {
        const value = await transport<NativeSnapshot>("install_cuda_package", {
          operationId,
        });
        if (!disposed && ticket === revision) apply(value);
      } finally {
        if (installing === operationId) installing = null;
        revision++;
        await refresh().catch(() => {});
        if (!disposed && isInstalled()) await installed().catch(() => {});
      }
    },
    async cancel() {
      const operationId = state.operationId;
      if (disposed || !operationId || !cudaPackageBusy(state.phase)) return;
      const ticket = ++revision;
      const value = await transport<NativeSnapshot>("cancel_cuda_package", {
        operationId,
      });
      if (!disposed && ticket === revision) apply(value);
    },
    dispose() {
      disposed = true;
      revision++;
      clearInterval(timer);
      // Closing a renderer does not cancel a user's native installation.
    },
  };
}
