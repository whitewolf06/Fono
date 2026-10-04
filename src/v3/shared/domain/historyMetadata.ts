export type DictationBackend = "cpu" | "cuda" | "vulkan";
export interface DictationMetadata {
  recordingDurationMs?: number | null;
  generationDurationMs?: number | null;
  recognitionDurationMs?: number | null;
  modelLoadDurationMs?: number | null;
  processingDurationMs?: number | null;
  backend?: DictationBackend | null;
  model?: string | null;
  requestedAcceleration?: "auto" | "cpu" | "cuda" | "vulkan";
  language?: string;
  detectedLanguage?: string | null;
  dictationMode?: "standard" | "live";
  processingMode?:
    "off" | "raw" | "clean" | "format" | "command" | "task" | "formal";
  processingTrigger?: "automatic" | "manual";
  processingTranslation?: "en" | "ru" | "de" | "fr" | "es" | null;
  dictionaryEnabled?: boolean;
  demo?: boolean;
  legacy?: boolean;
}
