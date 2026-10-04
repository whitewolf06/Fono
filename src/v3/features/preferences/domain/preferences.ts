import type {
  Preferences,
  Section,
  QuickPanel,
} from "../../../shared/domain/contracts";
import { validateWakePhrase } from "./wakePhrase";
import { validateDictionary } from "../../../shared/domain/personalDictionary";
export const defaults: Preferences = {
  dictationMode: "standard",
  microphone: "system",
  model: "small",
  language: "ru",
  acceleration: "auto",
  dictionaryEnabled: false,
  dictionaryEntries: [],
  wakeEnabled: true,
  wakePhrase: "Эй, фоно",
  wakeLanguage: "ru",
  silenceMs: 1600,
  hotkey: "Ctrl + Space",
  hotkeyMode: "hold",
  commandHotkey: "Ctrl + Shift + Space",
  wakeThreshold: 0.55,
  speechThreshold: 0.4,
  processingEnabled: true,
  processingMode: "clean",
  processingTrigger: "automatic",
  processingTranslation: "none",
  profile: "local",
  processingModel: "Qwen 3 · 8B",
  instruction: "",
  autostart: false,
  insertion: "clipboard",
  overlayEnabled: true,
  overlayCompact: false,
  overlayScale: 100,
  overlayOpacity: 95,
  overlayPosition: "bottom",
  historyEnabled: true,
  trainerEnabled: false,
  analyticsConsent: false,
  retentionDays: 30,
  trainerAiEnabled: false,
  trainerProfile: "local",
  trainerModel: "Qwen 3 · 8B",
  trainerScope: "metrics_only",
  cloudConsent: false,
  serviceEnabled: true,
  verboseLogging: false,
};
export const sectionKeys: Record<Section, (keyof Preferences)[]> = {
  general: ["autostart", "insertion"],
  audio: [
    "microphone",
    "model",
    "language",
    "acceleration",
    "dictionaryEntries",
  ],
  activation: [
    "dictationMode",
    "wakeEnabled",
    "wakePhrase",
    "wakeLanguage",
    "silenceMs",
    "hotkey",
    "hotkeyMode",
    "commandHotkey",
    "wakeThreshold",
    "speechThreshold",
  ],
  processing: [
    "processingEnabled",
    "processingMode",
    "processingTrigger",
    "processingTranslation",
    "profile",
    "processingModel",
    "instruction",
    "trainerAiEnabled",
    "trainerProfile",
    "trainerModel",
    "trainerScope",
    "cloudConsent",
  ],
  overlay: [
    "overlayEnabled",
    "overlayCompact",
    "overlayScale",
    "overlayOpacity",
    "overlayPosition",
  ],
  privacy: [
    "historyEnabled",
    "trainerEnabled",
    "analyticsConsent",
    "retentionDays",
  ],
  diagnostics: ["verboseLogging"],
};
export const quickKeys: Record<QuickPanel, (keyof Preferences)[]> = {
  microphone: ["microphone"],
  wake: ["wakePhrase", "wakeLanguage", "silenceMs"],
  recognition: ["model", "language", "acceleration"],
  processing: [
    "processingMode",
    "processingTrigger",
    "processingTranslation",
    "profile",
    "processingModel",
  ],
  hotkey: ["hotkey", "hotkeyMode", "dictationMode"],
  "command-hotkey": ["commandHotkey"],
};
export const quickSections: Record<QuickPanel, Section> = {
  microphone: "audio",
  recognition: "audio",
  wake: "activation",
  processing: "processing",
  hotkey: "activation",
  "command-hotkey": "activation",
};
export const microphones = [
  { value: "system", label: "Системный микрофон" },
  { value: "usb", label: "Студийный микрофон · USB Audio" },
  { value: "headset", label: "Гарнитура · Realtek Audio" },
];
export const languages = [
  { value: "ru", label: "Русский" },
  { value: "auto", label: "Определять автоматически" },
  { value: "en", label: "Английский" },
];
export const accelerations = [
  { value: "auto", label: "Авто · рекомендуется" },
  { value: "cpu", label: "Процессор" },
  { value: "cuda", label: "NVIDIA CUDA" },
  { value: "vulkan", label: "Vulkan" },
];
export const profiles = [
  { value: "local", label: "На компьютере · LM Studio" },
  { value: "cloud", label: "В облаке · OpenAI (демо)" },
];
export function validatePreferences(p: Preferences): string | null {
  const dictionaryError = validateDictionary(p.dictionaryEntries);
  if (dictionaryError) return dictionaryError;
  if (!["hold", "toggle"].includes(p.hotkeyMode))
    return "Выберите режим горячей клавиши.";
  if (!["automatic", "manual"].includes(p.processingTrigger))
    return "Выберите способ запуска обработки.";
  if (!["clean", "format", "task", "formal"].includes(p.processingMode))
    return "Выберите режим обработки.";
  if (!["none", "en", "ru", "de", "fr", "es"].includes(p.processingTranslation))
    return "Выберите язык перевода.";
  if (
    !/^(Ctrl|Alt|Shift)(\s\+\s(Ctrl|Alt|Shift))*\s\+\s([A-Z0-9]|Space|F[1-9]|F1[0-2])$/.test(
      p.hotkey,
    )
  )
    return "Укажите сочетание, например Ctrl + Space или Alt + F9.";
  if (p.hotkey === p.commandHotkey)
    return "Клавиши диктовки и команд должны отличаться.";
  if (
    !/^(Ctrl|Alt|Shift)(\s\+\s(Ctrl|Alt|Shift))*\s\+\s([A-Z0-9]|Space|F[1-9]|F1[0-2])$/.test(
      p.commandHotkey,
    )
  )
    return "Укажите корректное сочетание для голосовых команд.";
  const wakeError = validateWakePhrase(p.wakePhrase, p.wakeLanguage);
  if (wakeError) return wakeError;
  if (!["standard", "live"].includes(p.dictationMode))
    return "Выберите обычную или живую диктовку.";
  if (p.silenceMs < 600 || p.silenceMs > 5000)
    return "Пауза должна быть от 600 до 5000 мс.";
  if (p.trainerEnabled && !p.analyticsConsent)
    return "Для тренера нужно согласие на локальное хранение исходных расшифровок.";
  if (p.trainerAiEnabled && p.trainerProfile === "cloud" && !p.cloudConsent)
    return "Разрешите отправку выбранных данных в облако или выберите локальное подключение.";
  return null;
}
