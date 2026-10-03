import { reactive } from "vue";
import type { UpdatesPort, UpdateStatus } from "../../../shared/domain/updates";
import { updateBusy } from "../../../shared/domain/updates";

export function createMockUpdates(version: string): UpdatesPort {
  const next = version.split(".").map(Number);
  next[2]++;
  const state = reactive<UpdateStatus>({
    phase: "idle",
    currentVersion: version,
    downloadedBytes: 0,
    message:
      "Демонстрация обновлений: файлы не скачиваются и не устанавливаются.",
    checksEnabled: false,
  });
  let revision = 0;
  const pause = () => new Promise<void>((resolve) => setTimeout(resolve, 80));
  return {
    state,
    async refresh() {},
    async check() {
      if (updateBusy(state.phase)) throw new Error("Проверка уже выполняется");
      const id = ++revision;
      state.phase = "checking";
      await pause();
      if (revision !== id) return;
      state.phase = "available";
      state.nextVersion = next.join(".");
      state.message = "Доступно демонстрационное обновление.";
    },
    async install() {
      if (state.phase !== "available")
        throw new Error("Сначала проверьте обновления");
      const id = ++revision;
      state.phase = "downloading";
      state.totalBytes = 100;
      for (let bytes = 0; bytes <= 100; bytes += 20) {
        await pause();
        if (id !== revision) return;
        state.downloadedBytes = bytes;
      }
      state.phase = "up_to_date";
      state.message = "Демонстрация завершена. Установщик не запускался.";
      state.nextVersion = null;
    },
    async cancel() {
      if (state.phase !== "downloading") return;
      revision++;
      state.phase = "available";
      state.downloadedBytes = 0;
      state.message = "Демонстрационная загрузка отменена.";
    },
    async setChecksEnabled(enabled) {
      state.checksEnabled = enabled;
    },
    dispose() {
      revision++;
    },
  };
}
