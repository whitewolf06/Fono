import type { DictationHistoryEntry } from "./ipcTypes";
import type {
  DictationMetadata,
  DictationBackend,
  LocalDictationBackend,
} from "../../domain/historyMetadata";

function actualBackend(device: string | null): DictationBackend | undefined {
  if (legacyRemoteDevice(device)) return "remote";
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

function legacyRemoteDevice(device: unknown): string | undefined {
  if (typeof device !== "string" || !device.startsWith("remote:")) return;
  return remoteDevice(device.slice(7));
}
function remoteDevice(value: unknown): string | undefined {
  return typeof value === "string" &&
    /^(?:cpu|cuda(?::\d{1,3})?|vulkan)$/.test(value)
    ? value
    : undefined;
}
function localBackend(value: unknown): LocalDictationBackend | undefined {
  return value === "cpu" || value === "cuda" || value === "vulkan"
    ? value
    : undefined;
}
function knownBackend(value: unknown): DictationBackend | undefined {
  return value === "remote" ? "remote" : localBackend(value);
}
function serverName(value: unknown): string | undefined {
  if (
    typeof value !== "string" ||
    !value.trim() ||
    value.length > 80 ||
    [...value].some(
      (character) =>
        character.charCodeAt(0) < 32 || character.charCodeAt(0) === 127,
    ) ||
    /[\\/@]|[a-z][a-z0-9+.-]*:\/\//i.test(value)
  )
    return;
  return value.trim();
}
function measured(value: unknown): number | undefined {
  return typeof value === "number" &&
    Number.isSafeInteger(value) &&
    value >= 0 &&
    value <= 86_400_000
    ? value
    : undefined;
}

export function historyMetadataFromNative(
  entry: DictationHistoryEntry,
): DictationMetadata | undefined {
  const m = entry.metadata;
  if (m) {
    const backend =
      m.backend == null && legacyRemoteDevice(entry.device)
        ? "remote"
        : knownBackend(m.backend);
    const remote = backend === "remote";
    let device = remote
      ? m.remote_device == null
        ? m.backend == null
          ? legacyRemoteDevice(entry.device)
          : undefined
        : remoteDevice(m.remote_device)
      : undefined;
    let physical = remote ? localBackend(m.remote_backend) : undefined;
    if (device && physical && localBackend(device.split(":")[0]) !== physical) {
      device = undefined;
      physical = undefined;
    }
    const request = remote ? measured(m.request_duration_ms) : undefined;
    const serverTotal = remote
      ? measured(m.server_total_duration_ms)
      : undefined;
    let decode = remote ? measured(m.decode_duration_ms) : undefined;
    let network = remote ? measured(m.network_duration_ms) : undefined;
    if (decode != null && serverTotal != null && decode > serverTotal)
      decode = undefined;
    if (network != null && request != null && network > request)
      network = undefined;
    return {
      fallback: m.fallback === true,
      speechTiming:
        entry.analytics_included !== false &&
        entry.original_text &&
        m.speech_timing
          ? {
              schemaVersion: m.speech_timing.schema_version,
              speechTimeMs: m.speech_timing.speech_time_ms,
              pauseCount: m.speech_timing.pause_count,
              pauseTimeMs: m.speech_timing.pause_time_ms,
              maxPauseMs: m.speech_timing.max_pause_ms,
            }
          : undefined,
      recordingDurationMs: m.recording_duration_ms,
      generationDurationMs: m.generation_duration_ms,
      recognitionDurationMs: remote ? decode : m.recognition_duration_ms,
      modelLoadDurationMs: m.model_load_duration_ms,
      processingDurationMs: m.processing_duration_ms,
      backend,
      remoteName: remote ? serverName(m.remote_name) : undefined,
      remoteDevice: device,
      remoteBackend: physical,
      requestDurationMs: request,
      serverTotalDurationMs: serverTotal,
      networkDurationMs: network,
      decodeDurationMs: decode,
      recognizer: m.recognizer,
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
  }
  // Old archives may have the actual backend and opt-in processing statistics.
  // Preserve what exists; never infer missing model/settings from today's UI.
  const backend = actualBackend(entry.device);
  if (!backend && !entry.processing) return undefined;
  return {
    legacy: true,
    backend,
    remoteDevice:
      backend === "remote" ? legacyRemoteDevice(entry.device) : undefined,
    remoteBackend:
      backend === "remote"
        ? localBackend(legacyRemoteDevice(entry.device)?.split(":")[0])
        : undefined,
    recordingDurationMs:
      entry.processing?.audio_secs != null
        ? entry.processing.audio_secs * 1000
        : null,
    recognitionDurationMs:
      backend !== "remote" && entry.processing?.transcribe_secs != null
        ? entry.processing.transcribe_secs * 1000
        : null,
    detectedLanguage: entry.processing?.detected_language,
    processingMode: entry.processing?.ai_mode,
  };
}
