import { reactive } from "vue";
import type { UpdatesPort, UpdateStatus } from "../../../shared/domain/updates";
import { updateBusy } from "../../../shared/domain/updates";
import { call } from "../../../shared/infrastructure/native/ipc";

export function createNativeUpdates(
  version: string,
  transport = call,
  settingsTransaction?: (action: () => Promise<void>) => Promise<void>,
): UpdatesPort {
  const state = reactive<UpdateStatus>({
    phase: "idle",
    currentVersion: version,
    downloadedBytes: 0,
    message: "Загрузка состояния обновлений…",
    checksEnabled: false,
  });
  let disposed = false;
  let revision = 0;
  let activeActions = 0;
  let readSequence = 0;
  let pendingRead:
    { revision: number; id: number; promise: Promise<void> } | undefined;
  function apply(value: UpdateStatus) {
    Object.assign(state, { nextVersion: null, totalBytes: null }, value);
  }
  function refresh(): Promise<void> {
    if (disposed) return Promise.resolve();
    const ticket = revision;
    if (pendingRead?.revision === ticket) return pendingRead.promise;
    const id = ++readSequence;
    const promise = (async () => {
      try {
        const value = await transport<UpdateStatus>("get_update_status");
        if (
          !disposed &&
          ticket === revision &&
          id === readSequence &&
          (!activeActions || updateBusy(value.phase))
        )
          apply(value);
      } catch (error) {
        // An older startup/poll failure cannot replace a newer action's result.
        if (!disposed && ticket === revision && id === readSequence)
          throw error;
      }
    })();
    pendingRead = { revision: ticket, id, promise };
    void promise
      .finally(() => {
        if (pendingRead?.id === id) pendingRead = undefined;
      })
      .catch(() => {});
    return promise;
  }
  async function action(command: string, args?: Record<string, unknown>) {
    const ticket = ++revision;
    activeActions++;
    try {
      const value = await transport<UpdateStatus>(command, args);
      if (!disposed && ticket === revision) apply(value);
    } finally {
      activeActions--;
      revision++;
      // Keep download progress polling while install_update is pending, but
      // never turn a completed write into an error of its diagnostic refresh.
      await refresh().catch(() => {});
    }
  }
  const timer = setInterval(() => {
    void refresh().catch(() => {});
  }, 2000);
  void refresh().catch(() => {
    state.phase = "error";
    state.message =
      "Не удалось прочитать состояние обновлений. Повторите проверку.";
  });
  return {
    state,
    refresh,
    check: () => action("check_for_updates"),
    install: () => action("install_update"),
    cancel: () => action("cancel_update_download"),
    setChecksEnabled: (enabled) => {
      const write = () => action("set_update_checks_enabled", { enabled });
      return settingsTransaction ? settingsTransaction(write) : write();
    },
    dispose() {
      disposed = true;
      revision++;
      clearInterval(timer);
    },
  };
}
