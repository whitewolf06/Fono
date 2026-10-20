import type {
  Preferences,
  Section,
  QuickPanel,
} from "../../../shared/domain/contracts";
import { validateWakePhrase } from "./wakePhrase";
import { message } from "../../../shared/domain/messages";
import { validateDictionary } from "../../../shared/domain/personalDictionary";
import { triggerConflict } from "../../../shared/domain/triggers";
import {
  emptyProcessingPrompts,
  validateProcessingPrompts,
} from "./processingPrompts";
export const defaults: Preferences = {
  pretranscriptionEnabled: false,
  dictationMode: "standard",
  microphone: "system",
  model: "small",
  language: "ru",
  acceleration: "auto",
  whisperDecoding: "fast",
  whisperThreads: null,
  gpuModelResidency: "resident",
  dictionaryEnabled: false,
  dictionaryEntries: [],
  wakeEnabled: false,
  wakePhrase: "Эй, фоно",
  wakeLanguage: "ru",
  silenceMs: 1600,
  hotkey: "Ctrl + Space",
  dictationTrigger: "shortcut",
  selectionCaptureEnabled: false,
  selectionHotkey: "Ctrl + Alt + Space",
  selectionClipboardFallback: false,
  hotkeyMode: "hold",
  commandHotkey: "Ctrl + Shift + Space",
  wakeThreshold: 0.55,
  speechThreshold: 0.4,
  processingEnabled: true,
  processingMode: "clean",
  processingTrigger: "automatic",
  processingTranslation: "none",
  processingTranslationEnabled: true,
  processingPrompts: emptyProcessingPrompts(),
  profile: "local",
  processingModel: "Qwen 3 · 8B",
  instruction: "",
  autostart: false,
  insertion: "clipboard",
  overlayEnabled: true,
  overlayCompact: false,
  overlayQuickProcessing: true,
  overlayScale: 100,
  overlayOpacity: 95,
  overlayPosition: "bottom",
  historyEnabled: true,
  historyRetentionDays: 365,
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
    "whisperDecoding",
    "whisperThreads",
    "gpuModelResidency",
    "dictionaryEntries",
  ],
  activation: [
    "dictationMode",
    "wakeEnabled",
    "wakePhrase",
    "wakeLanguage",
    "silenceMs",
    "hotkey",
    "dictationTrigger",
    "selectionCaptureEnabled",
    "selectionHotkey",
    "selectionClipboardFallback",
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
    "processingTranslationEnabled",
    "processingPrompts",
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
    "overlayQuickProcessing",
    "overlayScale",
    "overlayOpacity",
    "overlayPosition",
    "processingEnabled",
    "processingMode",
    "processingTranslation",
    "processingTranslationEnabled",
  ],
  privacy: [
    "historyEnabled",
    "historyRetentionDays",
    "trainerEnabled",
    "analyticsConsent",
    "retentionDays",
  ],
  diagnostics: ["verboseLogging"],
};
export const quickKeys: Record<QuickPanel, (keyof Preferences)[]> = {
  microphone: ["microphone"],
  wake: ["wakePhrase", "wakeLanguage", "silenceMs"],
  recognition: [
    "model",
    "language",
    "acceleration",
    "gpuModelResidency",
    "whisperDecoding",
    "whisperThreads",
  ],
  processing: [
    "processingMode",
    "processingTrigger",
    "processingTranslation",
    "processingTranslationEnabled",
    "profile",
    "processingModel",
  ],
  hotkey: [
    "hotkey",
    "dictationTrigger",
    "hotkeyMode",
    "dictationMode",
    "commandHotkey",
  ],
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
  if (!["fast", "accurate"].includes(p.whisperDecoding))
    return message("decoding.invalidMode");
  if (
    p.whisperThreads !== null &&
    (!Number.isInteger(p.whisperThreads) ||
      p.whisperThreads < 1 ||
      p.whisperThreads > 8)
  )
    return message("decoding.invalidThreads");
  if (!["resident", "adaptive"].includes(p.gpuModelResidency))
    return "Выберите режим хранения модели в видеопамяти.";
  if (typeof p.overlayQuickProcessing !== "boolean")
    return "Выберите, показывать ли быстрые настройки обработки.";
  const dictionaryError = validateDictionary(p.dictionaryEntries);
  if (dictionaryError) return dictionaryError;
  if (!["hold", "toggle"].includes(p.hotkeyMode))
    return "Выберите режим горячей клавиши.";
  if (!["automatic", "manual"].includes(p.processingTrigger))
    return "Выберите способ запуска обработки.";
  if (!["raw", "clean", "format", "task", "formal"].includes(p.processingMode))
    return "Выберите режим обработки.";
  if (typeof p.processingTranslationEnabled !== "boolean")
    return "Выберите, включён ли перевод.";
  const promptsError = validateProcessingPrompts(p.processingPrompts);
  if (promptsError) return promptsError;
  if (!["none", "en", "ru", "de", "fr", "es"].includes(p.processingTranslation))
    return "Выберите язык перевода.";
  const conflict = triggerConflict(p);
  if (conflict) return conflict;
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
