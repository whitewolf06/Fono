import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  BuildInfo,
  DeviceInfo,
  AccelerationCapabilities,
  PipelineMode,
  PipelineState,
  Settings,
  Transcript,
  WakeWordDiagnostics,
  WakeDictationCountdown,
  WakeWordRecognitionReport,
  WakeWordSampleReport,
  WakeWordTestReport,
  WhisperModelInfo,
  LocalTranscriptionJob,
  LocalTranscriptionServiceSnapshot,
} from "./types";

// ====== Синхронные команды ======

export const ipc = {
  getBuildInfo: () => invoke<BuildInfo>("get_build_info"),
  getLocalTranscriptionServiceSnapshot: () =>
    invoke<LocalTranscriptionServiceSnapshot>(
      "get_local_transcription_service_snapshot",
    ),
  cancelLocalTranscriptionJob: (id: string) =>
    invoke<LocalTranscriptionJob | null>("cancel_local_transcription_job", {
      id,
    }),
  clearLocalTranscriptionHistory: () =>
    invoke<void>("clear_local_transcription_history"),
  copyLocalTranscriptionApiToken: () =>
    invoke<void>("copy_local_transcription_api_token"),
  // Состояние конвейера
  getPipelineState: () => invoke<PipelineState>("get_pipeline_state"),
  startDictation: () => invoke<void>("start_dictation"),
  stopDictation: () => invoke<Transcript>("stop_dictation"),
  getDictationHistory: () =>
    invoke<import("./types").DictationHistoryEntry[]>("get_dictation_history"),
  clearDictationHistory: () => invoke<void>("clear_dictation_history"),
  deleteDictationHistoryEntry: (id: string) =>
    invoke<void>("delete_dictation_history_entry", { id }),
  getSpeechSessionAnalysis: (id: string) =>
    invoke<import("./types").SpeechSessionAnalysis | null>(
      "get_speech_session_analysis",
      { id },
    ),
  getSpeechPeriodReport: (from: string, to: string) =>
    invoke<import("./types").SpeechPeriodReport>("get_speech_period_report", {
      from,
      to,
    }),
  copyDictationText: (text: string) =>
    invoke<void>("copy_dictation_text", { text }),
  reinsertDictation: (text: string) =>
    invoke<void>("reinsert_dictation", { text }),
  getPendingVoiceCommand: () =>
    invoke<string | null>("get_pending_voice_command"),
  getPendingCommandProposal: () =>
    invoke<import("./types").CommandProposal | null>(
      "get_pending_command_proposal",
    ),
  getWakeWordCapabilities: () =>
    invoke<import("./types").WakeWordCapabilities>(
      "get_wake_word_capabilities",
    ),
  confirmVoiceCommand: () => invoke<string>("confirm_voice_command"),
  cancelVoiceCommand: () => invoke<void>("cancel_voice_command"),

  // Тестовая запись фиксированной длительности.
  // inject=true — вставить распознанный текст в активное окно (Этап 2).
  transcribeTest: (durationMs: number, inject?: boolean) =>
    invoke<Transcript>("transcribe_test", { durationMs, inject }),

  // Аудио
  listAudioDevices: () => invoke<DeviceInfo[]>("list_audio_devices"),

  // Whisper-модели
  listWhisperModels: () => invoke<WhisperModelInfo[]>("list_whisper_models"),
  downloadWhisperModel: (size: string) =>
    invoke<void>("download_whisper_model", { size }),
  setWhisperModel: (path: string) =>
    invoke<void>("set_whisper_model", { path }),

  // LLM
  testLlmConnection: () => invoke<string>("test_llm_connection"),
  listLlmModels: () => invoke<string[]>("list_llm_models"),

  // Настройки
  getSettings: () => invoke<Settings>("get_settings"),
  getAccelerationCapabilities: () =>
    invoke<AccelerationCapabilities>("get_acceleration_capabilities"),
  saveSettings: (settings: Settings) =>
    invoke<void>("save_settings", { settings }),

  // Overlay
  saveOverlayPosition: (x: number, y: number) =>
    invoke<void>("save_overlay_position", { x, y }),

  // Диагностика
  getRecentLogs: (lines?: number) =>
    invoke<string>("get_recent_logs", { lines: lines ?? 80 }),
  testMicrophone: (durationMs: number) =>
    invoke<MicTestResult>("test_microphone", { durationMs }),

  // Wake word
  getWakeWordStatus: () => invoke<string>("get_wake_word_status"),
  getWakeWordDiagnostics: () =>
    invoke<WakeWordDiagnostics | null>("get_wake_word_diagnostics"),
  testWakeWordModel: () => invoke<WakeWordTestReport>("test_wake_word_model"),
  recordWakeWordSample: (durationMs = 4000) =>
    invoke<WakeWordSampleReport>("record_wake_word_sample", { durationMs }),
  recognizeWakeWordSample: () =>
    invoke<WakeWordRecognitionReport>("recognize_wake_word_sample"),
  enableWakeWord: () => invoke<void>("enable_wake_word"),
  disableWakeWord: () => invoke<void>("disable_wake_word"),
  isKwsModelDownloaded: () => invoke<boolean>("is_kws_model_downloaded"),
  downloadKwsModel: () => invoke<void>("download_kws_model"),

  // Подтверждение диктовки (кнопка ✓ в оверлее)
  confirmDictation: () => invoke<void>("confirm_dictation"),

  // Отмена диктовки (кнопка Stop в оверлее)
  cancelDictation: () => invoke<void>("cancel_dictation"),

  // Логи
  clearLogs: () => invoke<void>("clear_logs"),
};

export interface MicTestResult {
  samples: number;
  duration_ms: number;
  /** Пиковый уровень 0..1 (1 = clipping). */
  peak: number;
  /** RMS уровень 0..1. */
  rms: number;
}

// ====== События (односторонние, из Rust -> JS) ======

export function onPipelineStateChange(
  handler: (state: PipelineState) => void,
): Promise<UnlistenFn> {
  return listen<PipelineState>("pipeline-state", (e) => handler(e.payload));
}

export function onError(handler: (msg: string) => void): Promise<UnlistenFn> {
  return listen<string>("error", (e) => handler(e.payload));
}

export function onWakeWordDetected(
  handler: (transcription: string) => void,
): Promise<UnlistenFn> {
  return listen<string>("wake-word-detected", (e) => handler(e.payload));
}

export function onWakeWordStatus(
  handler: (status: string) => void,
): Promise<UnlistenFn> {
  return listen<string>("wake-word-status", (e) => handler(e.payload));
}

export function onCommandResult(
  handler: (result: string) => void,
): Promise<UnlistenFn> {
  return listen<string>("command-result", (e) => handler(e.payload));
}

export function onCommandProposal(
  handler: (command: string) => void,
): Promise<UnlistenFn> {
  return listen<string>("command-proposal", (event) => handler(event.payload));
}

export function onPipelineMode(
  handler: (mode: PipelineMode) => void,
): Promise<UnlistenFn> {
  return listen<PipelineMode>("pipeline-mode", (e) => handler(e.payload));
}

export function onWakeDictationCountdown(
  handler: (countdown: WakeDictationCountdown) => void,
): Promise<UnlistenFn> {
  return listen<WakeDictationCountdown>("wake-dictation-countdown", (e) =>
    handler(e.payload),
  );
}

export function onSettingsChange(
  handler: (settings: Settings) => void,
): Promise<UnlistenFn> {
  return listen<Settings>("settings-changed", (e) => handler(e.payload));
}

export function onKwsModelDownloaded(
  handler: (ok: boolean) => void,
): Promise<UnlistenFn> {
  return listen<boolean>("kws-model-downloaded", (e) => handler(e.payload));
}

export function onLocalTranscriptionServiceChanged(
  handler: () => void,
): Promise<UnlistenFn> {
  return listen("local-transcription-service-changed", () => handler());
}
