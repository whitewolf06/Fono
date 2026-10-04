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
  { value: "raw", label: "Без изменений" },
  { value: "clean", label: "Очистка" },
  { value: "format", label: "Структура" },
  { value: "task", label: "Задача" },
  { value: "formal", label: "Деловое письмо" },
] as const;
export const translationOptions = [
  { value: "none", label: "Без перевода" },
  { value: "en", label: "Английский" },
  { value: "ru", label: "Русский" },
  { value: "de", label: "Немецкий" },
  { value: "fr", label: "Французский" },
  { value: "es", label: "Испанский" },
] as const;
