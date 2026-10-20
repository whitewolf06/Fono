import type { Phase, WorkspaceState } from "./contracts";
import { message } from "./messages";

export interface PretranscriptionStatus {
  operation_id: number;
  available: boolean;
  phase:
    | "inactive"
    | "waiting"
    | "listening"
    | "queued"
    | "recognizing"
    | "fallback";
  completed_segments: number;
  start_after_ms: number;
}

export function captureSettingsLocked(phase: Phase): boolean {
  return [
    "listening",
    "silence",
    "transcribing",
    "processing",
    "awaiting_action",
  ].includes(phase);
}

export function pretranscriptionMessage(
  state: WorkspaceState,
  native: boolean,
): string {
  if (state.pending.pretranscriptionEnabled)
    return message("pretranscription.saving");
  if (!native) return message("pretranscription.demo");
  const status = state.pretranscription;
  if (!status) return message("pretranscription.unknown");
  if (!status.available) return message("pretranscription.unavailable");
  if (!state.preferences.pretranscriptionEnabled)
    return message("pretranscription.off");
  if (state.phase === "awaiting_action")
    return message("pretranscription.ready");
  if (state.phase !== "listening" && state.phase !== "silence") {
    return message(
      captureSettingsLocked(state.phase)
        ? "pretranscription.finishing"
        : "pretranscription.idle",
    );
  }
  return message(`pretranscription.${status.phase}`, {
    seconds: status.start_after_ms / 1000,
    count: status.completed_segments,
  });
}
