// Shared типы между frontend и backend.
// В Rust они зеркалируются в src-tauri/src/types.rs (serde::Serialize).

export type PipelineState =
  | "idle"
  | "listening"
  | "transcribing"
  | "processing"
  | "awaiting_action"
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
export type LlmConnectionKind = "local" | "cloud";
export type SpeechLlmDataScope = "metrics_only" | "findings" | "original_text";
export interface LlmProfile {
  id: string;
  name: string;
  provider: LlmProvider;
  connection: LlmConnectionKind;
  base_url: string;
  model: string | null;
  /** Never returned after a saved setting. */
  api_key: string | null;
  has_api_key: boolean;
}
export interface LlmConsumerAssignment {
  profile_id: string | null;
  model: string | null;
}
export interface SpeechLlmAssignment extends LlmConsumerAssignment {
  enabled: boolean;
  data_scope: SpeechLlmDataScope;
  cloud_consent: boolean;
}
export type PipelineMode = "dictation" | "command";

/** Таймер тишины для диктовки, которая запущена ключевой фразой. */
export interface WakeDictationCountdown {
  remaining_ms: number;
  timeout_ms: number;
  speaking: boolean;
}
export type WakeWordBackend =
  | "disabled"
  | "whisper_experimental"
  | "sherpa_onnx"
  | "sherpa_streaming_ru"
  | "sherpa_streaming_en"
  | "mock";
export interface WakeWordCapabilities {
  backend: WakeWordBackend;
  supports_custom_phrase: boolean;
  supported_phrases: string[];
  includes_pre_roll: boolean;
  supported_languages?: ("ru" | "en")[];
  available_languages?: ("ru" | "en")[];
}

export interface Settings {
  /** Обычная диктовка или последовательная вставка устойчивых фрагментов. */
  dictation_mode: "standard" | "live";
  /** device_id микрофона или null = системный default */
  audio_device_id: string | null;
  /** Путь к Whisper-модели (.bin) */
  whisper_model_path: string | null;
  /** Язык распознавания ("auto", "ru", "en", ...) */
  language: string;
  /** Optional local final-text replacements, never applied to raw transcripts. */
  personal_dictionary_enabled: boolean;
  personal_dictionary_entries: { written: string; spoken: string[] }[];
  update_checks_enabled: boolean;
  /** Глобальная горячая клавиша push-to-talk, напр. "Ctrl+Space" */
  hotkey: string;
  hotkey_mode?: "hold" | "toggle";
  processing_workflow?: "automatic" | "manual";
  processing_preset?: "clean" | "format" | "task" | "formal" | null;
  processing_target_language?: "en" | "ru" | "de" | "fr" | "es" | null;
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
  /** Local aggregate-only wake-word calibration, if one has completed. */
  wake_calibration_profile?: WakeCalibrationProfile | null;
  /** Режим AI-постобработки */
  ai_mode: AiMode;
  /** URL локального LLM-сервера (LM Studio) */
  llm_base_url: string;
  /** Имя модели в LM Studio (или null = первая доступная) */
  llm_model: string | null;
  /** Автозапуск с Windows */
  autostart: boolean;
  service_enabled: boolean;
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
  /** Показывать overlay во время диктовки */
  overlay_enabled: boolean;
  /** Масштаб overlay-окна */
  overlay_scale: number;
  /** Прозрачность overlay-окна (0..1) */
  overlay_opacity: number;
  /** Мини-режим overlay */
  overlay_mini_mode: boolean;
  /** Быстрый выбор стиля и перевода в индикаторе режима повторного нажатия. */
  overlay_quick_processing?: boolean;
  /** Подробные логи для отладки */
  verbose_logging: boolean;
  /** Провайдер LLM */
  llm_provider: LlmProvider;
  /** API-ключ для облачного LLM */
  llm_api_key: string | null;
  has_llm_api_key: boolean;
  llm_profiles: LlmProfile[];
  text_correction_llm: LlmConsumerAssignment;
  speech_analysis_llm: SpeechLlmAssignment;
  history_enabled: boolean;
  /** Явное согласие на локальное хранение исходного текста для аналитики речи. */
  analytics_enabled: boolean;
  /** Приостанавливает сбор и анализ новых сессий тренера, не затрагивая уже сохранённые данные. */
  speech_trainer_enabled: boolean;
  /** Срок хранения исходного текста и метаданных аналитики. */
  analytics_retention_days: number;
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

/** Aggregate-only result of a completed local wake-word calibration. */
export interface WakeCalibrationProfile {
  backend: WakeWordBackend;
  model_version: string;
  phrase: string;
  graph: string;
  threshold: number;
  sensitivity: number;
  vad_threshold: number;
  completed_at: string;
  accepted_samples: number;
  rejected_samples: number;
  average_rms: number;
  average_peak: number;
  average_active_ms: number;
  validation?: WakeCalibrationValidation | null;
}

/** Aggregate outcome of fresh post-registration Sherpa checks. */
export interface WakeCalibrationValidation {
  completed_at: string;
  positive_passed: number;
  positive_required: number;
  negative_passed: number;
  negative_required: number;
  confirmed_threshold: number;
}

export type WakeCalibrationRejection =
  "silence" | "clipping" | "too_short" | "phrase_not_detected";

export interface WakeCalibrationSampleResult {
  accepted: boolean;
  reason: WakeCalibrationRejection | null;
  rms: number;
  peak: number;
  active_ms: number;
  detected?: boolean;
  matched_candidates?: number;
}

export interface WakeCalibrationStatus {
  active: boolean;
  recording: boolean;
  required_samples: number;
  accepted_samples: number;
  rejected_samples: number;
  phrase: string;
  latest_result: WakeCalibrationSampleResult | null;
  profile: WakeCalibrationProfile | null;
}

export type WakeProfileValidationKind = "positive" | "silence" | "other_phrase";
export type WakeProfileValidationInputIssue =
  "silence" | "unexpected_speech" | "clipping" | "too_short";

export interface WakeProfileValidationSampleResult {
  kind: WakeProfileValidationKind;
  detected: boolean;
  accepted: boolean;
  input_issue: WakeProfileValidationInputIssue | null;
}

export interface WakeProfileValidationStatus {
  active: boolean;
  recording: boolean;
  failed: boolean;
  completed: boolean;
  positive_passed: number;
  positive_required: number;
  silence_passed: boolean;
  other_phrase_passed: boolean;
  negative_required: number;
  latest_result: WakeProfileValidationSampleResult | null;
}

export const DEFAULT_SETTINGS: Settings = {
  dictation_mode: "standard",
  audio_device_id: null,
  whisper_model_path: null,
  language: "auto",
  personal_dictionary_enabled: false,
  personal_dictionary_entries: [],
  update_checks_enabled: false,
  hotkey: "Ctrl+Space",
  wake_word_enabled: false,
  wake_word: "hey fono",
  wake_backend: "sherpa_onnx",
  wake_word_threshold: 0.25,
  wake_word_sensitivity: 0.5,
  wake_calibration_profile: null,
  ai_mode: "clean",
  llm_base_url: "http://localhost:1234/v1",
  llm_model: null,
  autostart: false,
  service_enabled: true,
  overlay_x: null,
  overlay_y: null,
  clean_prompt: null,
  acceleration: "auto",
  injection_mode: "sendinput",
  command_hotkey: "Ctrl+Shift+Space",
  launch_apps: [],
  overlay_scale: 1.0,
  overlay_enabled: true,
  overlay_opacity: 1.0,
  overlay_mini_mode: false,
  overlay_quick_processing: true,
  verbose_logging: false,
  llm_provider: "lmstudio",
  llm_api_key: null,
  has_llm_api_key: false,
  llm_profiles: [
    {
      id: "default",
      name: "Основной LLM",
      provider: "lmstudio",
      connection: "local",
      base_url: "http://localhost:1234/v1",
      model: null,
      api_key: null,
      has_api_key: false,
    },
  ],
  text_correction_llm: { profile_id: "default", model: null },
  speech_analysis_llm: {
    enabled: false,
    profile_id: null,
    model: null,
    data_scope: "metrics_only",
    cloud_consent: false,
  },
  history_enabled: true,
  analytics_enabled: false,
  speech_trainer_enabled: true,
  analytics_retention_days: 30,
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
  metadata?: DictationHistoryMetadata | null;
  analytics_included: boolean;
  original_text?: string | null;
  processing?: DictationProcessingMetadata | null;
  analysis_status: "disabled" | "pending" | "ready" | "failed" | "expired";
  analysis?: SpeechSessionAnalysis | null;
  analysis_error?: string | null;
  recommendation_status:
    "disabled" | "pending" | "ready" | "failed" | "expired";
  recommendation?: SpeechLlmRecommendation | null;
  recommendation_error?: string | null;
}

export interface DictationHistoryMetadata {
  schema_version: number;
  recording_duration_ms: number | null;
  generation_duration_ms: number | null;
  recognition_duration_ms: number | null;
  model_load_duration_ms: number | null;
  processing_duration_ms: number | null;
  backend: "cpu" | "cuda" | "vulkan" | null;
  model: string | null;
  requested_acceleration: AccelerationMode;
  language: string;
  detected_language: string | null;
  dictation_mode: "standard" | "live";
  processing_mode: AiMode;
  processing_workflow?: "automatic" | "manual" | null;
  processing_preset?: "clean" | "format" | "task" | "formal" | null;
  processing_target_language?: "en" | "ru" | "de" | "fr" | "es" | null;
  dictionary_enabled: boolean;
}

export interface SpeechLlmRecommendation {
  summary: string;
  recommendations: SpeechLlmRecommendationItem[];
}

export interface SpeechLlmRecommendationItem {
  title: string;
  observation: string;
  exercise: string;
  finding_indexes: number[];
}

export interface DictationProcessingMetadata {
  ai_mode: AiMode;
  detected_language: string | null;
  transcribe_secs: number | null;
  audio_secs: number | null;
}

export interface SpeechSessionAnalysis {
  word_count: number;
  filler_count: number;
  filler_density_per_100_words: number;
  repetition_count: number;
  self_correction_count: number;
  unfinished_count: number;
  findings: SpeechFinding[];
}

export interface SpeechFinding {
  kind: "filler" | "repetition" | "self_correction" | "unfinished";
  label: string;
  fragment: string;
  start_word: number;
  end_word: number;
}

export interface SpeechPeriodReport {
  from: string;
  to: string;
  analyzed_sessions: number;
  total_words: number;
  filler_count: number;
  repetition_count: number;
  self_correction_count: number;
  unfinished_count: number;
  filler_density_per_100_words: number;
  daily: SpeechDailyTrend[];
}

export interface SpeechDailyTrend {
  date: string;
  sessions: number;
  words: number;
  filler_count: number;
  repetition_count: number;
  self_correction_count: number;
  unfinished_count: number;
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
  requested_language: string;
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
