import { reactive } from "vue";
import type { UpdatesPort, UpdateStatus } from "../../../shared/domain/updates";
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
  let refreshing = false;
  async function refresh() {
    if (disposed || refreshing) return;
    refreshing = true;
    try {
      const value = await transport<UpdateStatus>("get_update_status");
      if (!disposed)
        Object.assign(state, { nextVersion: null, totalBytes: null }, value);
    } finally {
      refreshing = false;
    }
  }
  async function action(command: string, args?: Record<string, unknown>) {
    try {
      const value = await transport<UpdateStatus>(command, args);
      if (!disposed)
        Object.assign(state, { nextVersion: null, totalBytes: null }, value);
    } finally {
      await refresh();
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
      clearInterval(timer);
    },
  };
}
