import { message as uiMessage } from "../../../shared/domain/messages";
import type {
  SettingsPort,
  WorkspaceState,
} from "../../../shared/domain/contracts";
import { validatePreferences } from "../domain/preferences";
import { availableDictationMode } from "../../../shared/domain/dictationMode";
import {
  availableWakeEnabled,
  ensureWakeAvailable,
} from "../../../shared/domain/wakeAvailability";
import { mockProcessing } from "./mockProcessing";
import { captureSettingsLocked } from "../../../shared/domain/pretranscription";
import { clonePreferences } from "../domain/processingPrompts";
import {
  compatibleModelPreferences,
  validateModelPreferences,
} from "../../../shared/domain/speechModels";
import {
  persistPreferences,
  playDemoSample,
} from "../../../shared/infrastructure/browser";
const wait = (ms: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));
export function createSettingsPort(state: WorkspaceState): SettingsPort {
  async function save(patch: Partial<WorkspaceState["preferences"]>) {
    if (patch.wakeEnabled) ensureWakeAvailable();
    const guardCapture = () => {
      if (state.triggerCaptureActive)
        throw new Error(uiMessage("ui.13ccc97bac"));
      if (
        [
          "dictationMode",
          "dictationTrigger",
          "selectionCaptureEnabled",
          "selectionHotkey",
          "selectionClipboardFallback",
          "selectionAutoInsert",
          "hotkey",
          "hotkeyMode",
          "toggleAutoStopMs",
          "commandHotkey",
          "pretranscriptionEnabled",
          "model",
          "language",
          "acceleration",
          "whisperDecoding",
          "whisperThreads",
        ].some((key) => key in patch) &&
        [
          "listening",
          "silence",
          "transcribing",
          "processing",
          "awaiting_action",
        ].includes(state.phase)
      )
        throw new Error(uiMessage("ui.e134b483ff"));
    };
    guardCapture();
    await wait(420);
    guardCapture();
    if (state.scenario === "save-error")
      throw new Error(uiMessage("ui.a78fb4e919"));
    const next = {
      ...state.preferences,
      ...patch,
      dictationMode: availableDictationMode(patch.dictationMode),
      wakeEnabled: availableWakeEnabled(
        patch.wakeEnabled ?? state.preferences.wakeEnabled,
      ),
    };
    const selected = state.models.find((m) => m.id === next.model);
    if ("model" in patch)
      Object.assign(next, compatibleModelPreferences(selected, next));
    if (["model", "language", "acceleration"].some((key) => key in patch))
      validateModelPreferences(selected, next);
    const phraseChanged =
      next.wakePhrase !== state.preferences.wakePhrase ||
      next.wakeLanguage !== state.preferences.wakeLanguage;
    if (phraseChanged) next.wakeEnabled = false;
    const trainerProfile = state.profiles.find(
      (p) => p.id === next.trainerProfile,
    );
    if (
      next.trainerAiEnabled &&
      trainerProfile?.location === "cloud" &&
      !next.cloudConsent
    )
      throw new Error(uiMessage("ui.db8fb01c94"));
    const error = validatePreferences(next);
    if (error) throw new Error(error);
    persistPreferences(next);
    Object.assign(state.preferences, clonePreferences(next));
    if (!next.analyticsConsent || !next.trainerEnabled)
      for (const entry of state.history)
        if (entry.metadata) delete entry.metadata.speechTiming;
    if (phraseChanged && state.wakeSetup) {
      state.wakeSetup.profileReady = false;
      state.wakeSetup.verified = false;
    }
    state.logs.unshift(uiMessage("ui.d8ea53b00e"));
  }
  return {
    ...mockProcessing(state),
    save,
    async toggle(key, value) {
      if (state.pending[key]) return;
      if (
        key === "pretranscriptionEnabled" &&
        captureSettingsLocked(state.phase)
      )
        throw new Error(uiMessage("ui.e134b483ff"));
      if (key === "wakeEnabled" && value) ensureWakeAvailable();
      if (key === "wakeEnabled" && value && state.wakeSetup?.verified === false)
        throw new Error(uiMessage("ui.e3bf1feef8"));
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
      model.downloadPhase = "downloading";
      model.downloadError = "";
      for (let progress = 0; progress <= 100; progress += 10) {
        await wait(220);
        if (!state.models.includes(model) || model.status !== "downloading")
          return;
        model.progress = progress;
      }
      model.downloadPhase = "verifying";
      await wait(220);
      if (model.status !== "downloading") return;
      model.status = "installed";
      model.downloadPhase = undefined;
      state.logs.unshift(model.name + uiMessage("ui.47df730362"));
    },
    removeModel(id) {
      const model = state.models.find((m) => m.id === id);
      if (model) {
        if (state.preferences.model === id)
          throw new Error(uiMessage("ui.17beacba05"));
        model.status = "available";
        model.progress = 0;
      }
    },
    async testMicrophone() {
      if (!state.microphoneAvailable)
        throw new Error(uiMessage("ui.9010565737"));
      for (let i = 0; i < 16; i++) {
        state.testSignal = 0.2 + Math.abs(Math.sin(i * 0.9)) * 0.65;
        await wait(90);
      }
      state.testSignal = 0;
    },
    async saveProfile(profile) {
      await wait(350);
      if (!profile.name.trim() || !profile.model.trim())
        throw new Error(uiMessage("ui.7b221ba79c"));
      try {
        const url = new URL(profile.url);
        if (!["http:", "https:"].includes(url.protocol)) throw new Error();
      } catch {
        throw new Error(uiMessage("ui.71ee37667c"));
      }
      if (state.scenario === "save-error")
        throw new Error(uiMessage("ui.c8e7f4c482"));
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
        throw new Error(uiMessage("ui.fa8452ba07"));
      state.profiles = state.profiles.filter((p) => p.id !== id);
    },
    playSample: playDemoSample,
    async testConnection(profileId = state.preferences.profile) {
      await wait(850);
      if (!state.aiAvailable) throw new Error(uiMessage("ui.3bde9b3ecb"));
      return state.profiles.find((p) => p.id === profileId)?.location ===
        "local"
        ? uiMessage("ui.475e961539")
        : uiMessage("ui.e4c31da862");
    },
  };
}
