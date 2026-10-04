export type ProcessingPreset = "clean" | "format" | "task" | "formal";
export type ProcessingTrigger = "automatic" | "manual";
export type TranslationLanguage = "en" | "ru" | "de" | "fr" | "es";
export type ProcessingTranslation = "none" | TranslationLanguage;
export interface OverlayProcessingChoice {
  preset: ProcessingPreset;
  targetLanguage: TranslationLanguage | null;
}
export interface OverlayProcessingChoiceRequest extends OverlayProcessingChoice {
  sessionId: number | null;
}
export interface PendingDictation {
  sessionId: number;
  phase: "awaiting_action" | "processing";
  originalText: string;
  resultText: string | null;
  createdAt: string;
  preset: ProcessingPreset;
  targetLanguage: TranslationLanguage | null;
  processingEnabled: boolean;
  source: "ui" | "hotkey" | "wake_word";
  error: string | null;
  insertionBlocked: boolean;
}
export interface PendingDictationRequest {
  sessionId: number;
  action: "insert_raw" | "process_and_insert" | "cancel";
  preset?: ProcessingPreset;
  targetLanguage?: TranslationLanguage | null;
}
export const presetOptions = [
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
