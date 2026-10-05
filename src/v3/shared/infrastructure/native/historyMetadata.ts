import type { DictationHistoryEntry } from "./ipcTypes";
import type {
  DictationMetadata,
  DictationBackend,
} from "../../domain/historyMetadata";

function actualBackend(device: string | null): DictationBackend | undefined {
  switch (device) {
    case "CPU":
      return "cpu";
    case "CUDA":
      return "cuda";
    case "Vulkan":
      return "vulkan";
    default:
      return undefined;
  }
}

export function historyMetadataFromNative(
  entry: DictationHistoryEntry,
): DictationMetadata | undefined {
  const m = entry.metadata;
  if (m)
    return {
      recordingDurationMs: m.recording_duration_ms,
      generationDurationMs: m.generation_duration_ms,
      recognitionDurationMs: m.recognition_duration_ms,
      modelLoadDurationMs: m.model_load_duration_ms,
      processingDurationMs: m.processing_duration_ms,
      backend: m.backend,
      model: m.model,
      requestedAcceleration: m.requested_acceleration,
      language: m.language,
      detectedLanguage: m.detected_language,
      dictationMode: m.dictation_mode,
      processingMode: m.processing_preset ?? m.processing_mode,
      processingTrigger: m.processing_workflow ?? undefined,
      processingTranslation: m.processing_target_language,
      dictionaryEnabled: m.dictionary_enabled,
    };
  // Old archives may have the actual backend and opt-in processing statistics.
  // Preserve what exists; never infer missing model/settings from today's UI.
  const backend = actualBackend(entry.device);
  if (!backend && !entry.processing) return undefined;
  return {
    legacy: true,
    backend,
    recordingDurationMs:
      entry.processing?.audio_secs != null
        ? entry.processing.audio_secs * 1000
        : null,
    recognitionDurationMs:
      entry.processing?.transcribe_secs != null
        ? entry.processing.transcribe_secs * 1000
        : null,
    detectedLanguage: entry.processing?.detected_language,
    processingMode: entry.processing?.ai_mode,
  };
}
