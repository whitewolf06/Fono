import type {
  SettingsPort,
  WorkspaceState,
} from "../../../shared/domain/contracts";
import { validatePreferences } from "../domain/preferences";
import {
  persistPreferences,
  playDemoSample,
} from "../../../shared/infrastructure/browser";
const wait = (ms: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));
export function createSettingsPort(state: WorkspaceState): SettingsPort {
  async function save(patch: Partial<WorkspaceState["preferences"]>) {
    await wait(420);
    if (state.scenario === "save-error")
      throw new Error(
        "Не удалось сохранить. Изменения остались в форме — попробуйте ещё раз после восстановления соединения.",
      );
    const next = { ...state.preferences, ...patch };
    const trainerProfile = state.profiles.find(
      (p) => p.id === next.trainerProfile,
    );
    if (
      next.trainerAiEnabled &&
      trainerProfile?.location === "cloud" &&
      !next.cloudConsent
    )
      throw new Error(
        "Для облачного разбора требуется разрешение на передачу выбранных данных.",
      );
    const error = validatePreferences(next);
    if (error) throw new Error(error);
    persistPreferences(next);
    Object.assign(state.preferences, next);
    state.logs.unshift("Настройки обновлены");
  }
  return {
    save,
    async toggle(key, value) {
      if (state.pending[key]) return;
      const previous = state.preferences[key];
      state.pending[key] = true;
      state.preferences[key] = value;
      try {
        await save({ [key]: value });
      } catch (error) {
        state.preferences[key] = previous;
        throw error;
      } finally {
        state.pending[key] = false;
      }
    },
    async downloadModel(id) {
      const model = state.models.find((m) => m.id === id);
      if (!model || model.status !== "available") return;
      model.status = "downloading";
      for (let progress = 0; progress <= 100; progress += 10) {
        await wait(220);
        if (!state.models.includes(model) || model.status !== "downloading")
          return;
        model.progress = progress;
      }
      model.status = "installed";
      state.logs.unshift(model.name + ": демонстрационная загрузка завершена");
    },
    removeModel(id) {
      const model = state.models.find((m) => m.id === id);
      if (model) {
        model.status = "available";
        model.progress = 0;
      }
    },
    async testMicrophone() {
      if (!state.microphoneAvailable)
        throw new Error(
          "Микрофон не найден. Подключите устройство и повторите проверку.",
        );
      for (let i = 0; i < 16; i++) {
        state.testSignal = 0.2 + Math.abs(Math.sin(i * 0.9)) * 0.65;
        await wait(90);
      }
      state.testSignal = 0;
    },
    async saveProfile(profile) {
      await wait(350);
      if (!profile.name.trim() || !profile.model.trim())
        throw new Error("Укажите название профиля и модель.");
      try {
        const url = new URL(profile.url);
        if (!["http:", "https:"].includes(url.protocol)) throw new Error();
      } catch {
        throw new Error("Укажите корректный http или https адрес.");
      }
      if (state.scenario === "save-error")
        throw new Error(
          "Не удалось сохранить профиль. Попробуйте после восстановления соединения.",
        );
      const next = { ...profile, id: profile.id || "profile-" + Date.now() };
      const index = state.profiles.findIndex((p) => p.id === next.id);
      if (index < 0) state.profiles.push(next);
      else state.profiles[index] = next;
    },
    removeProfile(id) {
      if (
        state.preferences.profile === id ||
        state.preferences.trainerProfile === id
      )
        throw new Error("Сначала выберите другое подключение.");
      state.profiles = state.profiles.filter((p) => p.id !== id);
    },
    playSample: playDemoSample,
    async testConnection(profileId = state.preferences.profile) {
      await wait(850);
      if (!state.aiAvailable)
        throw new Error(
          "Модель не отвечает. Проверьте подключение в разделе «Обработка текста и ИИ».",
        );
      return state.profiles.find((p) => p.id === profileId)?.location ===
        "local"
        ? "Демонстрационное подключение доступно · 24 мс"
        : "Демонстрационное облачное подключение доступно · 186 мс";
    },
  };
}
