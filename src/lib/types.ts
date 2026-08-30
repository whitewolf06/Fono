// Shared типы между frontend и backend.
// В Rust они зеркалируются в src-tauri/src/types.rs (serde::Serialize).

export type PipelineState =
  "idle" | "listening" | "transcribing" | "processing" | "injecting" | "error";

export interface DeviceInfo {
  id: string;
  name: string;
  is_default: boolean;
}

export interface WhisperModelInfo {
  /** Имя файла, напр. "ggml-base.bin" */
  filename: string;
  /** Размер: tiny / base / small / medium / large / large_turbo */
  size: WhisperModelSize;
  /** Локальный путь, если модель уже скачана */
  local_path: string | null;
  /** Размер файла в байтах (для отображения прогресса скачивания) */
  bytes: number | null;
}

export type WhisperModelSize =
  "tiny" | "base" | "small" | "medium" | "large" | "large_turbo";

export type AiMode = "off" | "clean" | "format" | "command";
export type InjectionMode = "sendinput" | "clipboard";
export type AccelerationMode = "auto" | "cuda" | "vulkan" | "cpu";
export interface AccelerationCapabilities {
  cuda: boolean;
  vulkan: boolean;
}
export type LlmProvider = "lmstudio" | "openai" | "custom";
export type PipelineMode = "dictation" | "command";

/** Таймер тишины для диктовки, которая запущена ключевой фразой. */
export interface WakeDictationCountdown {
  remaining_ms: number;
  timeout_ms: number;
  speaking: boolean;
}
export type WakeWordBackend =
  "disabled" | "whisper_experimental" | "sherpa_onnx" | "mock";
export interface WakeWordCapabilities {
  backend: WakeWordBackend;
  supports_custom_phrase: boolean;
  supported_phrases: string[];
  includes_pre_roll: boolean;
}

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
  /** Backend wake word */
  wake_backend: WakeWordBackend;
  /** Порог срабатывания wake word (backend-specific) */
  wake_word_threshold: number;
  /** Чувствительность / boosting score wake word */
  wake_word_sensitivity: number;
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
  /** Предпочтительный backend ускорения Whisper */
  acceleration: AccelerationMode;
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
  has_llm_api_key: boolean;
  history_enabled: boolean;
  /** Модель whisper для wake word */
  wake_word_model: WhisperModelSize;
  /** Порог VAD для wake word (чувствительность) */
  wake_word_vad_threshold: number;
  /** Пауза после речи, завершающая диктовку по wake word. */
  wake_dictation_silence_ms: number;
  /** RMS-порог тишины для завершения диктовки после wake word. */
  wake_dictation_speech_threshold: number;
  /** Шаг изменения громкости в процентах */
  volume_step: number;
}

export interface LaunchApp {
  name: string;
  exe_path: string;
  aliases: string[];
}

export interface CommandProposal {
  id: number;
  operation_id: number;
  source: "ui" | "hotkey" | "wake_word" | "diagnostic";
  original_text: string;
  normalized_action: string;
  confidence: number | null;
  created_at: string;
  expires_at: string;
  settings_version: number;
}

export interface WakeWordDiagnostics {
  running: boolean;
  paused: boolean;
  frames_received: number;
  rms: number;
  peak: number;
  last_event: string;
  last_result_keyword: string;
  last_result_json: string;
}

export interface WakeWordTestReport {
  detected: boolean;
  keyword: string;
  json: string;
  duration_ms: number;
}

export interface WakeWordSampleReport {
  samples: number;
  duration_ms: number;
  rms: number;
  peak: number;
}

export interface WakeWordRecognitionReport {
  backend: string;
  detected: boolean;
  recognized: string;
  json: string;
  audio_duration_ms: number;
  processing_ms: number;
}

export const DEFAULT_SETTINGS: Settings = {
  audio_device_id: null,
  whisper_model_path: null,
  language: "auto",
  hotkey: "Ctrl+Space",
  wake_word_enabled: false,
  wake_word: "hey fono",
  wake_backend: "sherpa_onnx",
  wake_word_threshold: 0.25,
  wake_word_sensitivity: 0.5,
  ai_mode: "clean",
  llm_base_url: "http://localhost:1234/v1",
  llm_model: null,
  autostart: false,
  overlay_x: null,
  overlay_y: null,
  clean_prompt: null,
  acceleration: "auto",
  injection_mode: "sendinput",
  command_hotkey: "Ctrl+Shift+Space",
  launch_apps: [],
  overlay_scale: 1.0,
  overlay_opacity: 1.0,
  overlay_mini_mode: false,
  verbose_logging: false,
  llm_provider: "lmstudio",
  llm_api_key: null,
  has_llm_api_key: false,
  history_enabled: true,
  wake_word_model: "base",
  wake_word_vad_threshold: 0.015,
  wake_dictation_silence_ms: 2000,
  wake_dictation_speech_threshold: 0.006,
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

export interface DictationHistoryEntry {
  id: string;
  text: string;
  created_at: string;
  device: string | null;
}
export interface BuildInfo {
  version: string;
  revision: string;
  profile: string;
}

/** Raw IPC contract for the non-persistent local REST-service monitor. */
export type LocalTranscriptionJobState =
  | "queued"
  | "preparing"
  | "transcribing"
  | "completed"
  | "failed"
  | "cancelled";

export interface LocalTranscriptionResult {
  protocol_version: number;
  text: string;
  detected_language: string | null;
  audio_seconds: number | null;
  transcribe_seconds: number | null;
  model: string;
  backend: string | null;
}

export interface LocalTranscriptionJob {
  id: string;
  state: LocalTranscriptionJobState;
  created_at_ms: number;
  started_at_ms: number | null;
  finished_at_ms: number | null;
  result: LocalTranscriptionResult | null;
  error: { code: string; message: string } | null;
}

export interface LocalTranscriptionQueueSnapshot {
  capacity: number;
  queued: number;
  preparing: number;
  transcribing: number;
  completed: number;
  failed: number;
  cancelled: number;
  jobs: LocalTranscriptionJob[];
}

export interface LocalTranscriptionHistorySnapshot {
  jobs: LocalTranscriptionJob[];
  completed: number;
  failed: number;
  cancelled: number;
  total_audio_seconds: number;
  total_transcribe_seconds: number;
}

export interface LocalTranscriptionServiceSnapshot {
  address: string;
  protocol_version: number;
  queue: LocalTranscriptionQueueSnapshot;
  history: LocalTranscriptionHistorySnapshot;
}
