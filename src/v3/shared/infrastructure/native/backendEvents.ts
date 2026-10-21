import type { PipelineState, Settings } from "./ipcTypes";
import type { GpuMemoryStatus } from "../../domain/gpuMemory";
import {
  isDictationOutcomeEvent,
  type DictationOutcomeEvent,
} from "../../domain/dictationOutcome";

interface Payloads {
  dictation_outcome: DictationOutcomeEvent;
  operation: {
    kind: "phase_changed" | "finished";
    id: number;
    source: "ui" | "hotkey" | "wake_word" | "diagnostics" | "service";
    phase: string;
    reason?: string;
  };
  pipeline_state: { state: PipelineState };
  pipeline_mode: "dictation" | "command";
  wake:
    | { type: "status"; status: string }
    | { type: "detected"; phrase: string }
    | {
        type: "countdown";
        remaining_ms: number;
        timeout_ms: number;
        speaking: boolean;
      };
  settings: { settings: Settings };
  stt_readiness: {
    readiness:
      | { state: "unloaded" | "loading" }
      | { state: "ready"; device: string }
      | { state: "failed"; message: string };
  };
  model_download: {
    download_id: string;
    model: string;
    phase:
      | "started"
      | "downloading"
      | "verifying"
      | "extracting"
      | "completed"
      | "cancelled"
      | "failed";
    // Terminal failure also survives a detached download IPC waiter.
    downloaded_bytes: number;
    total_bytes?: number | null;
  };
  service: null;
  history_changed: null;
  gpu_memory_changed: GpuMemoryStatus;
  audio_level: {
    auto_stop?: import("../../domain/toggleAutoStop").ToggleAutoStopStatus;
    operation_id: number;
    level: number;
    elapsed_ms: number;
    pretranscription?: import("../../domain/pretranscription").PretranscriptionStatus;
  };
  error: { code: string; message: string; operation_id: number | null };
}
export type BackendEventV1 = {
  [K in keyof Payloads]: { schema_version: 1; kind: K; payload: Payloads[K] };
}[keyof Payloads];

const kinds = new Set<keyof Payloads>([
  "dictation_outcome",
  "operation",
  "pipeline_state",
  "pipeline_mode",
  "wake",
  "settings",
  "stt_readiness",
  "model_download",
  "service",
  "history_changed",
  "gpu_memory_changed",
  "audio_level",
  "error",
]);

/** Unknown protocol versions/kinds must not mutate the current UI. */
export function isBackendEventV1(value: unknown): value is BackendEventV1 {
  if (!value || typeof value !== "object") return false;
  const envelope = value as Record<string, unknown>;
  return (
    envelope.schema_version === 1 &&
    typeof envelope.kind === "string" &&
    kinds.has(envelope.kind as keyof Payloads) &&
    "payload" in envelope &&
    (envelope.kind !== "dictation_outcome" ||
      isDictationOutcomeEvent(envelope.payload))
  );
}
