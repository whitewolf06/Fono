import { message as uiMessage } from "../../../shared/domain/messages";
import { validateSnippets } from "../../snippets/domain/snippets";
import type {
  Preferences,
  Section,
  QuickPanel,
} from "../../../shared/domain/contracts";
import { validateWakePhrase } from "./wakePhrase";
import { validateApplicationProfiles } from "../../../shared/domain/applicationProfiles";
import { message } from "../../../shared/domain/messages";
import { validateDictionary } from "../../../shared/domain/personalDictionary";
import { triggerConflict } from "../../../shared/domain/triggers";
import {
  emptyProcessingPrompts,
  validateProcessingPrompts,
} from "./processingPrompts";
export const defaults: Preferences = {
  uiLanguage: "auto",
  applicationProfiles: [],
  snippetsEnabled: false,
  snippets: [],
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
  dictionaryLlmEnabled: false,
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
  selectionAutoInsert: false,
  selectionHistoryEnabled: false,
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
  general: ["uiLanguage", "autostart", "insertion", "applicationProfiles"],
  audio: [
    "microphone",
    "model",
    "language",
    "acceleration",
    "whisperDecoding",
    "whisperThreads",
    "gpuModelResidency",
    "dictionaryEntries",
    "snippets",
    "dictionaryLlmEnabled",
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
    "selectionAutoInsert",
    "selectionHistoryEnabled",
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
    "language",
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
  {
    value: "system",
    get label() {
      return uiMessage("ui.e32ad3f607");
    },
  },
  {
    value: "usb",
    get label() {
      return uiMessage("ui.069004675b");
    },
  },
  {
    value: "headset",
    get label() {
      return uiMessage("ui.f5f9d282ce");
    },
  },
];
export const languages = [
  {
    value: "ru",
    get label() {
      return uiMessage("ui.6da140fdd4");
    },
  },
  {
    value: "auto",
    get label() {
      return uiMessage("ui.ef8f09e724");
    },
  },
  {
    value: "en",
    get label() {
      return uiMessage("ui.e7c1ef1d8e");
    },
  },
];
export const accelerations = [
  {
    value: "auto",
    get label() {
      return uiMessage("ui.0cdf37c953");
    },
  },
  {
    value: "cpu",
    get label() {
      return uiMessage("ui.df6652413d");
    },
  },
  { value: "cuda", label: "NVIDIA CUDA" },
  { value: "vulkan", label: "Vulkan" },
];
export const profiles = [
  {
    value: "local",
    get label() {
      return uiMessage("ui.4dde67fe65");
    },
  },
  {
    value: "cloud",
    get label() {
      return uiMessage("ui.543e794f79");
    },
  },
];
export function validatePreferences(p: Preferences): string | null {
  if (!["auto", "ru", "en"].includes(p.uiLanguage))
    return uiMessage("ui.0359251343");
  const profileError = validateApplicationProfiles(p.applicationProfiles);
  if (profileError) return profileError;
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
    return uiMessage("ui.70697c971b");
  if (typeof p.overlayQuickProcessing !== "boolean")
    return uiMessage("ui.6c2e5590f3");
  const snippetError = validateSnippets(p.snippets);
  if (snippetError) return snippetError;
  const dictionaryError = validateDictionary(p.dictionaryEntries);
  if (dictionaryError) return dictionaryError;
  if (!["hold", "toggle"].includes(p.hotkeyMode))
    return uiMessage("ui.74a58caa86");
  if (!["automatic", "manual"].includes(p.processingTrigger))
    return uiMessage("ui.6fa2231be7");
  if (!["raw", "clean", "format", "task", "formal"].includes(p.processingMode))
    return uiMessage("ui.9e6ab24980");
  if (typeof p.processingTranslationEnabled !== "boolean")
    return uiMessage("ui.8551f7513b");
  const promptsError = validateProcessingPrompts(p.processingPrompts);
  if (promptsError) return promptsError;
  if (!["none", "en", "ru", "de", "fr", "es"].includes(p.processingTranslation))
    return uiMessage("ui.ddf776c712");
  const conflict = triggerConflict(p);
  if (conflict) return conflict;
  const wakeError = validateWakePhrase(p.wakePhrase, p.wakeLanguage);
  if (wakeError) return wakeError;
  if (!["standard", "live"].includes(p.dictationMode))
    return uiMessage("ui.3b20ef815e");
  if (p.silenceMs < 600 || p.silenceMs > 5000)
    return uiMessage("ui.518e51c2d4");
  if (p.trainerEnabled && !p.analyticsConsent)
    return uiMessage("ui.3db2e97273");
  if (p.trainerAiEnabled && p.trainerProfile === "cloud" && !p.cloudConsent)
    return uiMessage("ui.2e5bdc905c");
  return null;
}
