export type DictationOutcomeKind =
  "completed" | "deferred" | "no_speech" | "cancelled" | "superseded";

export interface DictationOutcomeEvent {
  operation_id: number;
  outcome: DictationOutcomeKind;
}

export function isDictationOutcomeEvent(
  value: unknown,
): value is DictationOutcomeEvent {
  if (!value || typeof value !== "object") return false;
  const event = value as Record<string, unknown>;
  return (
    Number.isSafeInteger(event.operation_id) &&
    Number(event.operation_id) > 0 &&
    ["completed", "deferred", "no_speech", "cancelled", "superseded"].includes(
      String(event.outcome),
    )
  );
}
