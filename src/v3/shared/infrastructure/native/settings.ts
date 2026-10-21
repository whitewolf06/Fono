import { message as uiMessage } from "../../domain/messages";
import type { SettingsPort, Preferences } from "../../domain/contracts";
import type { NativeContext } from "./context";
import type { LlmProfile } from "./ipcTypes";
import { call, playMicrophoneSample } from "./ipc";
import { nativeWake } from "./wake";
import { ensureWakeAvailable } from "../../domain/wakeAvailability";
import { modelId } from "../../domain/speechModels";
import { nativeProcessing } from "../../../features/preferences/infrastructure/nativeProcessing";
import { captureSettingsLocked } from "../../domain/pretranscription";
import { triggerConflict } from "../../domain/triggers";
export function nativeSettings(ctx: NativeContext): SettingsPort {
  const { state } = ctx;
  return {
    ...nativeProcessing(),
    async save(patch) {
      if (state.triggerCaptureActive)
        throw new Error(uiMessage("ui.13ccc97bac"));
      if (
        [
          "dictationTrigger",
          "hotkey",
          "hotkeyMode",
          "toggleAutoStopMs",
          "commandHotkey",
          "selectionHotkey",
          "selectionCaptureEnabled",
        ].some((key) => key in patch)
      ) {
        const conflict = triggerConflict({ ...state.preferences, ...patch });
        if (conflict) throw new Error(conflict);
      }
      if (patch.wakeEnabled) ensureWakeAvailable();
      if (
        [
          "dictationMode",
          "pretranscriptionEnabled",
          "gpuModelResidency",
          "whisperDecoding",
          "whisperThreads",
          "hotkey",
          "dictationTrigger",
          "selectionHotkey",
          "selectionCaptureEnabled",
          "selectionClipboardFallback",
          "selectionAutoInsert",
          "hotkeyMode",
          "commandHotkey",
          "processingTrigger",
          "processingMode",
          "processingTranslation",
          "processingTranslationEnabled",
          "processingPrompts",
          "profile",
          "processingModel",
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
      await ctx.save(patch);
      if (
        typeof __FONO_WAKE_WORD__ !== "undefined" &&
        __FONO_WAKE_WORD__ &&
        ["wakePhrase", "wakeLanguage", "wakeThreshold", "speechThreshold"].some(
          (key) => key in patch,
        )
      )
        await nativeWake(ctx).load();
    },
    async toggle(key, value) {
      if (
        [
          "selectionCaptureEnabled",
          "selectionClipboardFallback",
          "selectionAutoInsert",
        ].includes(key) &&
        (captureSettingsLocked(state.phase) || state.triggerCaptureActive)
      )
        throw new Error(uiMessage("ui.36d3ffdc1e"));
      if (state.pending[key]) return;
      if (
        key === "pretranscriptionEnabled" &&
        captureSettingsLocked(state.phase)
      )
        throw new Error(uiMessage("ui.e134b483ff"));
      if (key === "wakeEnabled" && value) ensureWakeAvailable();
      const previous = state.preferences[key];
      state.pending[key] = true;
      state.preferences[key] = value;
      try {
        if (key === "serviceEnabled") {
          await ctx.serialize(async () => {
            await call("set_local_transcription_service_enabled", {
              enabled: value,
            });
            await ctx.readSettings();
            await ctx.readService();
          });
        } else if (key === "wakeEnabled") {
          if (!(
            typeof __FONO_WAKE_WORD__ !== "undefined" && __FONO_WAKE_WORD__
          )) {
            await ctx.save({ wakeEnabled: false });
            state.wakeStatus = "off";
            return;
          }
          await ctx.serialize(async () => {
            await call(value ? "enable_wake_word" : "disable_wake_word");
            await ctx.readSettings();
            state.wakeStatus = await call<string>("get_wake_word_status");
            await nativeWake(ctx).load();
          });
        } else await ctx.save({ [key]: value } as Partial<Preferences>);
      } catch (error) {
        state.preferences[key] = previous;
        throw error;
      } finally {
        state.pending[key] = false;
      }
    },
    async downloadModel(id) {
      const model = state.models.find((m) => m.id === id);
      if (!model) throw new Error(uiMessage("ui.cf61c63359"));
      model.status = "downloading";
      model.progress = 0;
      model.downloadError = "";
      try {
        await call("download_stt_model", { id: modelId(id) });
      } catch (error) {
        model.downloadError =
          error instanceof Error ? error.message : String(error);
        throw error;
      } finally {
        model.status = "available";
        await ctx.readModels();
        const current = state.models.find((m) => m.id === id);
        if (current) current.downloadError = model.downloadError;
      }
    },
    async removeModel(id) {
      if (state.models.find((m) => m.id === id)?.status === "downloading")
        await call("cancel_model_download", {
          downloadId: id.includes(":") ? id : "whisper:" + id,
        });
      else await call("remove_stt_model", { id: modelId(id) });
      await ctx.readModels();
    },
    async testMicrophone(device) {
      const result = await call<{ rms: number; peak: number }>(
        "test_microphone_device",
        {
          deviceId:
            (device || state.preferences.microphone) === "system"
              ? null
              : device || state.preferences.microphone,
        },
      );
      state.testSignal = Math.min(1, result.rms * 8);
      await ctx.readDevices();
      if (result.peak < 0.005) throw new Error(uiMessage("ui.c0234902aa"));
    },
    playSample: playMicrophoneSample,
    listModels: (profileId) =>
      call<string[]>("list_llm_profile_models", { profileId }),
    async testConnection(profileId) {
      try {
        const result = await call<string>("test_llm_profile", {
          profileId: profileId || state.preferences.profile,
        });
        if (!profileId || profileId === state.preferences.profile) {
          state.aiAvailable = true;
          state.aiChecked = true;
        }
        return result;
      } catch (error) {
        if (!profileId || profileId === state.preferences.profile) {
          state.aiAvailable = false;
          state.aiChecked = true;
        }
        throw error;
      }
    },
    async saveProfile(profile) {
      if (!profile.name.trim() || !profile.url.trim())
        throw new Error(uiMessage("ui.1b0bbe970c"));
      const url = new URL(profile.url);
      if (!["http:", "https:"].includes(url.protocol))
        throw new Error(uiMessage("ui.fa25cefc58"));
      await ctx.saveRaw((current) => {
        const previous = current.llm_profiles.find((p) => p.id === profile.id);
        const next: LlmProfile = {
          id: profile.id || crypto.randomUUID(),
          name: profile.name,
          base_url: profile.url,
          provider: profile.provider as LlmProfile["provider"],
          connection: profile.location as LlmProfile["connection"],
          model: profile.model || null,
          api_key: profile.apiKey || null,
          has_api_key: previous?.has_api_key || false,
        };
        current.llm_profiles = [
          ...current.llm_profiles.filter((p) => p.id !== next.id),
          next,
        ];
        return current;
      });
    },
    async removeProfile(id) {
      await ctx.saveRaw((current) => {
        if (
          current.text_correction_llm.profile_id === id ||
          current.speech_analysis_llm.profile_id === id
        )
          throw new Error(uiMessage("ui.22ed0dc646"));
        current.llm_profiles = current.llm_profiles.filter((p) => p.id !== id);
        return current;
      });
    },
    async resetOverlay() {
      await call("reset_overlay_position");
    },
    async previewOverlay(p) {
      await call("show_overlay_preview", {
        overlayScale: p.overlayScale / 100,
        overlayOpacity: p.overlayOpacity / 100,
        overlayMiniMode: p.overlayCompact,
      });
    },
  };
}
