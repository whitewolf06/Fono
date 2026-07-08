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
export type InjectionMode = "sendinput" | "clipboard";
export type LlmProvider = "lmstudio" | "openai" | "custom";
export type PipelineMode = "dictation" | "command";

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
  /** Способ вставки текста в активное окно */
  injection_mode: InjectionMode;
  /** Горячая клавиша для голосовых команд */
  command_hotkey: string;
  /** Список приложений для запуска по голосовой команде */
  launch_apps: LaunchApp[];
  /** Масштаб overlay-окна */
  overlay_scale: number;
  /** Прозрачность overlay-окна (0..1) */
  overlay_opacity: number;
  /** Мини-режим overlay */
  overlay_mini_mode: boolean;
  /** Подробные логи для отладки */
  verbose_logging: boolean;
  /** Провайдер LLM */
  llm_provider: LlmProvider;
  /** API-ключ для облачного LLM */
  llm_api_key: string | null;
  /** Модель whisper для wake word */
  wake_word_model: WhisperModelSize;
  /** Порог VAD для wake word (чувствительность) */
  wake_word_vad_threshold: number;
  /** Шаг изменения громкости в процентах */
  volume_step: number;
}

export interface LaunchApp {
  name: string;
  exe_path: string;
  aliases: string[];
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
  injection_mode: "sendinput",
  command_hotkey: "Ctrl+Shift+Space",
  launch_apps: [],
  overlay_scale: 1.0,
  overlay_opacity: 1.0,
  overlay_mini_mode: false,
  verbose_logging: false,
  llm_provider: "lmstudio",
  llm_api_key: null,
  wake_word_model: "base",
  wake_word_vad_threshold: 0.015,
  volume_step: 10,
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
