import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  DeviceInfo,
  PipelineState,
  Settings,
  Transcript,
  WhisperModelInfo,
} from "./types";

// ====== Синхронные команды ======

export const ipc = {
  // Состояние конвейера
  getPipelineState: () => invoke<PipelineState>("get_pipeline_state"),
  startDictation: () => invoke<void>("start_dictation"),
  stopDictation: () => invoke<Transcript>("stop_dictation"),

  // Тестовая запись фиксированной длительности.
  // inject=true — вставить распознанный текст в активное окно (Этап 2).
  transcribeTest: (durationMs: number, inject?: boolean) =>
    invoke<Transcript>("transcribe_test", { durationMs, inject }),

  // Аудио
  listAudioDevices: () => invoke<DeviceInfo[]>("list_audio_devices"),

  // Whisper-модели
  listWhisperModels: () =>
    invoke<WhisperModelInfo[]>("list_whisper_models"),
  downloadWhisperModel: (size: string) =>
    invoke<void>("download_whisper_model", { size }),
  setWhisperModel: (path: string) =>
    invoke<void>("set_whisper_model", { path }),

  // LLM
  testLlmConnection: () => invoke<string>("test_llm_connection"),

  // Настройки
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) =>
    invoke<void>("save_settings", { settings }),

  // Диагностика
  getRecentLogs: (lines?: number) =>
    invoke<string>("get_recent_logs", { lines: lines ?? 80 }),
  testMicrophone: (durationMs: number) =>
    invoke<MicTestResult>("test_microphone", { durationMs }),
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
