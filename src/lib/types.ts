// Shared типы между frontend и backend.
// В Rust они зеркалируются в src-tauri/src/types.rs (serde::Serialize).

export type PipelineState =
  | "idle"
  | "listening"
  | "transcribing"
  | "processing"
  | "injecting"
  | "error";

export interface DeviceInfo {
  id: string;
  name: string;
  is_default: boolean;
}

export interface WhisperModelInfo {
  /** Имя файла, напр. "ggml-base.bin" */
  filename: string;
  /** Размер: tiny / base / small / medium / large */
  size: WhisperModelSize;
  /** Локальный путь, если модель уже скачана */
  local_path: string | null;
  /** Размер файла в байтах (для отображения прогресса скачивания) */
  bytes: number | null;
}

export type WhisperModelSize =
  | "tiny"
  | "base"
  | "small"
  | "medium"
  | "large";

export type AiMode = "off" | "clean" | "format" | "command";

export interface Settings {
  /** device_id микрофона или null = системный default */
  audio_device_id: string | null;
  /** Путь к Whisper-модели (.bin) */
  whisper_model_path: string | null;
  /** Язык распознавания ("auto", "ru", "en", ...) */
  language: string;
  /** Глобальная горячая клавиша push-to-talk, напр. "Ctrl+Space" */
  hotkey: string;
  /** Включена ли активация по ключевой фразе */
  wake_word_enabled: boolean;
  /** Сама фраза, напр. "Эй, ассистент" */
  wake_word: string;
  /** Режим AI-постобработки */
  ai_mode: AiMode;
  /** URL локального LLM-сервера (LM Studio) */
  llm_base_url: string;
  /** Имя модели в LM Studio (или null = первая доступная) */
  llm_model: string | null;
  /** Автозапуск с Windows */
  autostart: boolean;
  /** X-координата overlay-окна */
  overlay_x: number | null;
  /** Y-координата overlay-окна */
  overlay_y: number | null;
  /** Пользовательский системный промт для режима clean */
  clean_prompt: string | null;
  /** Использовать GPU (CUDA) для whisper, если доступно */
  use_gpu: boolean;
}

export const DEFAULT_SETTINGS: Settings = {
  audio_device_id: null,
  whisper_model_path: null,
  language: "auto",
  hotkey: "Ctrl+Space",
  wake_word_enabled: false,
  wake_word: "Эй, ассистент",
  ai_mode: "clean",
  llm_base_url: "http://localhost:1234/v1",
  llm_model: null,
  autostart: false,
  overlay_x: null,
  overlay_y: null,
  clean_prompt: null,
  use_gpu: true,
};

export interface Transcript {
  text: string;
  /** Язык, определённый whisper (если language = "auto") */
  detected_language: string | null;
  /** Время транскрипции в секундах */
  transcribe_secs?: number | null;
  /** Длительность аудио в секундах */
  audio_secs?: number | null;
  /** Какое устройство использовалось (CPU/CUDA) */
  device?: string | null;
}
