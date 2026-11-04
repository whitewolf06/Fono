import { message } from "./messages";

export interface RecoveryDictation {
  session_id: number;
  phase: "awaiting_action" | "recognizing";
  reason:
    "server_failed" | "server_unavailable" | "local_failed" | "choose_source";
  audio_secs: number;
  local_available: boolean;
}
export interface RecoveryRequest {
  session_id: number;
  action: "retry" | "recognize_local" | "cancel";
}
export function isRecoveryDictation(
  value: unknown,
): value is RecoveryDictation {
  if (!value || typeof value !== "object") return false;
  const item = value as Record<string, unknown>;
  return (
    Number.isSafeInteger(item.session_id) &&
    Number(item.session_id) > 0 &&
    ["awaiting_action", "recognizing"].includes(String(item.phase)) &&
    [
      "server_failed",
      "server_unavailable",
      "local_failed",
      "choose_source",
    ].includes(String(item.reason)) &&
    typeof item.audio_secs === "number" &&
    Number.isFinite(item.audio_secs) &&
    item.audio_secs > 0 &&
    item.audio_secs <= 300 &&
    typeof item.local_available === "boolean"
  );
}
export function recoveryTitle(value: RecoveryDictation): string {
  if (value.phase === "recognizing") return message("recovery.recognizing");
  switch (value.reason) {
    case "choose_source":
      return message("recovery.chooseSource");
    case "local_failed":
      return message("recovery.localFailed");
    case "server_unavailable":
      return message("recovery.serverUnavailable");
    default:
      return message("recovery.serverFailed");
  }
}
