import { ipc } from "@/lib/ipc";
import type { Settings } from "@/lib/types";
import {
  customWhisperModelSelectionLabel,
  whisperModelSelectionLabel,
  whisperModelSizeFromPath,
  whisperModelSizeFromSelection,
} from "@/v2/shared/domain/whisperModels";
import type {
  SettingsDraft,
  SettingsDraftStore,
} from "../application/useSettingsDraft";

export function createTauriSettingsDraftStore(): SettingsDraftStore {
  return {
    load: loadDraft,
    save: saveDraft,
    enableWakeWord: () => ipc.enableWakeWord(),
    testMicrophone: async () => ipc.testMicrophone(2000),
    downloadWhisperModel: downloadWhisperModel,
    testLmStudio: () => ipc.testLlmConnection(),
    showOverlayTest: (draft) =>
      ipc.showOverlayPreview({
        overlay_scale: draft.overlayScale / 100,
        overlay_opacity: draft.overlayOpacity / 100,
        overlay_mini_mode: draft.overlayMiniMode,
      }),
    resetOverlayPosition: () => ipc.resetOverlayPosition(),
  };
}

async function loadDraft(): Promise<SettingsDraft> {
  const [settings, devices, models, wakeCapabilities] = await Promise.all([
    ipc.getSettings(),
    ipc.listAudioDevices(),
    ipc.listWhisperModels(),
    ipc.getWakeWordCapabilities(),
  ]);
  const recognitionModel = recognitionModelLabel(settings.whisper_model_path);
  const recognitionModelOptions = models.map((model) =>
    whisperModelSelectionLabel(model.size),
  );

  if (!recognitionModelOptions.includes(recognitionModel)) {
    recognitionModelOptions.unshift(recognitionModel);
  }

  const supportsCustomWakePhrase =
    wakeCapabilities.backend === "whisper_experimental" &&
    wakeCapabilities.supports_custom_phrase;
  const wakePhraseIsSupported =
    supportsCustomWakePhrase ||
    wakeCapabilities.supported_phrases.some(
      (phrase) => phrase.toLowerCase() === settings.wake_word.toLowerCase(),
    );

  return {
    language: settings.language,
    insertionMode: settings.injection_mode,
    autostart: settings.autostart,
    hotkey: settings.hotkey,
    microphone: microphoneLabel(settings, devices),
    microphoneOptions: [
      "Default system device",
      ...devices.map((device) => device.name),
    ],
    recognitionModel,
    recognitionModelOptions,
    acceleration: settings.acceleration,
    wakeWordEnabled: settings.wake_word_enabled,
    wakePhrase: settings.wake_word,
    wakePhraseOptions: wakeCapabilities.supported_phrases,
    wakePhraseIsSupported,
    supportsCustomWakePhrase,
    wakeSensitivity: Math.round(settings.wake_word_sensitivity * 100),
    silenceDelay: settings.wake_dictation_silence_ms / 1000,
    processingEnabled: settings.ai_mode !== "off",
    processingMode: settings.ai_mode === "format" ? "format" : "clean",
    historyEnabled: settings.history_enabled,
    analyticsEnabled: settings.analytics_enabled,
    analyticsRetentionDays: settings.analytics_retention_days,
    overlayVisible: settings.overlay_enabled,
    overlayScale: Math.round(settings.overlay_scale * 100),
    overlayOpacity: Math.round(settings.overlay_opacity * 100),
    overlayMiniMode: settings.overlay_mini_mode,
    verboseLogging: settings.verbose_logging,
  };
}

async function saveDraft(draft: SettingsDraft): Promise<void> {
  const [currentSettings, devices] = await Promise.all([
    ipc.getSettings(),
    ipc.listAudioDevices(),
  ]);
  // A personal phrase must be configured and validated while the live
  // listener is off. Stop it before saving the new phrase so Rust validates
  // the pending settings as disabled rather than rejecting the transition.
  if (currentSettings.wake_word_enabled && !draft.wakeWordEnabled) {
    await ipc.disableWakeWord();
  }
  const settings = {
    ...currentSettings,
    wake_word_enabled:
      currentSettings.wake_word_enabled && draft.wakeWordEnabled,
  };
  const whisperModelPath = await resolveModelPath(
    draft.recognitionModel,
    settings.whisper_model_path,
  );
  const nextSettings = {
    ...settings,
    audio_device_id: resolveDeviceId(draft.microphone, devices),
    whisper_model_path: whisperModelPath,
    language: draft.language,
    hotkey: draft.hotkey.replaceAll(" ", ""),
    wake_word: draft.wakePhrase,
    wake_word_sensitivity: draft.wakeSensitivity / 100,
    wake_dictation_silence_ms: Math.round(draft.silenceDelay * 1000),
    ai_mode: draft.processingEnabled ? draft.processingMode : "off",
    history_enabled: draft.historyEnabled,
    analytics_enabled: draft.analyticsEnabled,
    analytics_retention_days: draft.analyticsRetentionDays,
    autostart: draft.autostart,
    acceleration: draft.acceleration,
    injection_mode: draft.insertionMode,
    overlay_scale: draft.overlayScale / 100,
    overlay_enabled: draft.overlayVisible,
    overlay_opacity: draft.overlayOpacity / 100,
    overlay_mini_mode: draft.overlayMiniMode,
    verbose_logging: draft.verboseLogging,
  } satisfies Settings;

  await ipc.saveSettings(nextSettings);

  if (draft.wakeWordEnabled && !currentSettings.wake_word_enabled) {
    await ipc.enableWakeWord();
  }
}

async function downloadWhisperModel(recognitionModel: string): Promise<void> {
  const size = whisperModelSizeFromSelection(recognitionModel);
  if (!size) {
    throw new Error(`Модель ${recognitionModel} не поддерживается.`);
  }

  await ipc.downloadWhisperModel(size);
}

function microphoneLabel(
  settings: Settings,
  devices: Awaited<ReturnType<typeof ipc.listAudioDevices>>,
) {
  if (!settings.audio_device_id) return "Default system device";

  return (
    devices.find((device) => device.id === settings.audio_device_id)?.name ??
    "Default system device"
  );
}

function resolveDeviceId(
  microphone: string,
  devices: Awaited<ReturnType<typeof ipc.listAudioDevices>>,
) {
  if (microphone === "Default system device") return null;

  const device = devices.find((candidate) => candidate.name === microphone);
  if (!device) throw new Error(`Микрофон «${microphone}» недоступен.`);

  return device.id;
}

function recognitionModelLabel(path: string | null) {
  const size = whisperModelSizeFromPath(path);
  if (size) return whisperModelSelectionLabel(size);
  if (path) return customWhisperModelSelectionLabel(path);
  return whisperModelSelectionLabel("small");
}

async function resolveModelPath(
  recognitionModel: string,
  currentPath: string | null,
) {
  if (recognitionModel === recognitionModelLabel(currentPath))
    return currentPath;

  const size = whisperModelSizeFromSelection(recognitionModel);
  if (!size) {
    throw new Error(`Модель ${recognitionModel} не поддерживается.`);
  }

  const models = await ipc.listWhisperModels();
  const model = models.find(
    (candidate) => candidate.size === size && candidate.local_path,
  );

  if (!model?.local_path) {
    throw new Error(`Модель ${recognitionModel} ещё не скачана.`);
  }

  return model.local_path;
}
