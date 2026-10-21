import { message as uiMessage } from "./messages";
export type ProcessingPreset = "raw" | "clean" | "format" | "task" | "formal";
export type PromptPreset = Exclude<ProcessingPreset, "raw">;
export interface ProcessingPromptChoice {
  useCustom: boolean;
  customPrompt: string;
}
export type ProcessingPrompts = Record<PromptPreset, ProcessingPromptChoice>;
export interface ProcessingPromptCatalog {
  presets: { preset: PromptPreset; label: string; defaultPrompt: string }[];
  maxPromptChars: number;
  maxTextBytes: number;
}
export interface ProcessingPreviewInput {
  text: string;
  preset: ProcessingPreset;
  targetLanguage: TranslationLanguage | null;
  promptOverride?: ProcessingPromptChoice;
}
export interface ProcessingPreviewResult {
  text: string;
  model: string | null;
  elapsedMs: number;
  preset: ProcessingPreset;
  targetLanguage: TranslationLanguage | null;
}
export type ProcessingTrigger = "automatic" | "manual";
export type TranslationLanguage = "en" | "ru" | "de" | "fr" | "es";
export type ProcessingTranslation = "none" | TranslationLanguage;
export interface OverlayProcessingChoice {
  recognitionLanguage?: string;
  preset: ProcessingPreset;
  targetLanguage: TranslationLanguage | null;
  processingEnabled?: boolean;
  translationEnabled?: boolean;
}
export interface OverlayProcessingChoiceRequest extends OverlayProcessingChoice {
  sessionId: number | null;
}
export interface PendingDictation {
  copyOnly?: boolean;
  sessionId: number;
  phase: "awaiting_action" | "processing";
  originalText: string;
  resultText: string | null;
  createdAt: string;
  preset: ProcessingPreset;
  targetLanguage: TranslationLanguage | null;
  processingEnabled: boolean;
  translationEnabled?: boolean;
  source: "ui" | "hotkey" | "wake_word";
  error: string | null;
  insertionBlocked: boolean;
}
export function effectiveProcessingLanguage(
  choice: OverlayProcessingChoice,
): TranslationLanguage | null {
  return choice.processingEnabled === false ||
    choice.translationEnabled === false
    ? null
    : choice.targetLanguage;
}
export interface PendingDictationRequest {
  sessionId: number;
  action:
    | "insert_raw"
    | "process_and_insert"
    | "process_preview"
    | "complete"
    | "cancel";
  preset?: ProcessingPreset;
  targetLanguage?: TranslationLanguage | null;
}
export const presetOptions = [
  {
    value: "raw",
    get label() {
      return uiMessage("ui.4274a06c5d");
    },
  },
  {
    value: "clean",
    get label() {
      return uiMessage("ui.b0f05e6f3d");
    },
  },
  {
    value: "format",
    get label() {
      return uiMessage("ui.816655bd71");
    },
  },
  {
    value: "task",
    get label() {
      return uiMessage("ui.b9ce27191d");
    },
  },
  {
    value: "formal",
    get label() {
      return uiMessage("ui.f7c565c215");
    },
  },
] as const;
export const translationOptions = [
  {
    value: "none",
    get label() {
      return uiMessage("ui.c29017610e");
    },
  },
  {
    value: "en",
    get label() {
      return uiMessage("ui.e7c1ef1d8e");
    },
  },
  {
    value: "ru",
    get label() {
      return uiMessage("ui.6da140fdd4");
    },
  },
  {
    value: "de",
    get label() {
      return uiMessage("ui.79c7568c22");
    },
  },
  {
    value: "fr",
    get label() {
      return uiMessage("ui.7518ed2eef");
    },
  },
  {
    value: "es",
    get label() {
      return uiMessage("ui.5097d628b8");
    },
  },
] as const;
