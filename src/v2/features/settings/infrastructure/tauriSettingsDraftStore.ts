import { ipc } from "@/lib/ipc";
import type { Settings } from "@/lib/types";
import type {
  SettingsDraft,
  SettingsDraftStore,
} from "../application/useSettingsDraft";

export function createTauriSettingsDraftStore(): SettingsDraftStore {
  return {
    load: loadDraft,
    save: saveDraft,
  };
}

async function loadDraft(): Promise<SettingsDraft> {
  const [settings, devices] = await Promise.all([
    ipc.getSettings(),
    ipc.listAudioDevices(),
  ]);

  return {
    language: settings.language,
    insertionMode: settings.injection_mode,
    autostart: settings.autostart,
    hotkey: settings.hotkey,
    microphone: microphoneLabel(settings, devices),
    recognitionModel: recognitionModelLabel(settings.whisper_model_path),
    acceleration: settings.acceleration,
    wakeWordEnabled: settings.wake_word_enabled,
    wakePhrase: settings.wake_word,
    wakeSensitivity: Math.round(settings.wake_word_sensitivity * 100),
    silenceDelay: settings.wake_dictation_silence_ms / 1000,
    processingEnabled: settings.ai_mode !== "off",
    processingMode: settings.ai_mode === "format" ? "format" : "clean",
    overlayVisible: true,
    overlayScale: Math.round(settings.overlay_scale * 100),
    overlayOpacity: Math.round(settings.overlay_opacity * 100),
    overlayMiniMode: settings.overlay_mini_mode,
    verboseLogging: settings.verbose_logging,
  };
}

async function saveDraft(draft: SettingsDraft): Promise<void> {
  const [settings, devices] = await Promise.all([
    ipc.getSettings(),
    ipc.listAudioDevices(),
  ]);
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
    autostart: draft.autostart,
    acceleration: draft.acceleration,
    injection_mode: draft.insertionMode,
    overlay_scale: draft.overlayScale / 100,
    overlay_opacity: draft.overlayOpacity / 100,
    overlay_mini_mode: draft.overlayMiniMode,
    verbose_logging: draft.verboseLogging,
  } satisfies Settings;

  await ipc.saveSettings(nextSettings);

  if (draft.wakeWordEnabled !== settings.wake_word_enabled) {
    if (draft.wakeWordEnabled) await ipc.enableWakeWord();
    else await ipc.disableWakeWord();
  }
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
  const size = path?.match(/ggml-(tiny|base|small|medium|large)\.bin$/i)?.[1];
  return size ? `Whisper ${capitalize(size)}` : "Whisper Small";
}

async function resolveModelPath(
  recognitionModel: string,
  currentPath: string | null,
) {
  if (recognitionModel === recognitionModelLabel(currentPath))
    return currentPath;

  const size = recognitionModel.replace("Whisper ", "").toLocaleLowerCase();
  const models = await ipc.listWhisperModels();
  const model = models.find(
    (candidate) => candidate.size === size && candidate.local_path,
  );

  if (!model?.local_path) {
    throw new Error(`Модель ${recognitionModel} ещё не скачана.`);
  }

  return model.local_path;
}

function capitalize(value: string) {
  return `${value[0].toUpperCase()}${value.slice(1)}`;
}
