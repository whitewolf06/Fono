export interface ToggleAutoStopStatus {
  operation_id: number;
  phase: "inactive" | "listening" | "silence" | "stopping" | "unavailable";
  timeout_ms: number;
  remaining_ms: number;
}

export function validToggleAutoStop(value: number): boolean {
  return (
    Number.isInteger(value) &&
    (value === 0 || (value >= 2000 && value <= 10000))
  );
}

export function toggleAutoStopCountdown(
  status: ToggleAutoStopStatus | undefined,
  operation: number,
) {
  if (
    !status ||
    operation <= 0 ||
    status.operation_id !== operation ||
    !["listening", "silence", "stopping"].includes(status.phase) ||
    !validToggleAutoStop(status.timeout_ms) ||
    status.timeout_ms === 0 ||
    !Number.isFinite(status.remaining_ms)
  )
    return undefined;
  return {
    remaining_ms: Math.max(0, Math.min(status.timeout_ms, status.remaining_ms)),
    timeout_ms: status.timeout_ms,
    speaking: status.phase === "listening",
  };
}
