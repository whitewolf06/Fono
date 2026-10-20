import type { SelectionCapturePort } from "../domain/selection";

/** Cancel actual work first, keeping the source and any result for recovery. */
export function cancelSelection(port: SelectionCapturePort): Promise<void> {
  const current = port.state.current;
  if (!current) return Promise.resolve();
  const id = current.operationId;
  if (current.edit?.operationId)
    return port.cancelEdit(id, current.edit.operationId);
  if (
    ["armed", "replacing", "copying", "cancelling"].includes(
      current.delivery?.phase ?? "idle",
    )
  ) {
    const result = current.delivery?.resultId ?? current.edit?.resultId;
    if (result) return port.cancelDelivery(id, result);
  }
  return port.cancel(id);
}
