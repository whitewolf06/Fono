import { reactive } from "vue";
import type {
  SelectionCapturePort,
  SelectionSnapshot,
} from "../domain/selection";
import { createCoalescedRead } from "../../../shared/infrastructure/native/coalescedRead";
import catalog from "../../../shared/infrastructure/native/userErrorCatalog.json";
type Call = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
type Subscribe = (
  channel: string,
  handler: (value: unknown) => void,
) => Promise<() => void>;

export function selectionFromNative(value: unknown): SelectionSnapshot | null {
  if (!value || typeof value !== "object") return null;
  const s = value as Record<string, unknown>;
  if (
    !Number.isSafeInteger(s.operation_id) ||
    Number(s.operation_id) <= 0 ||
    !["reading", "ready", "error"].includes(String(s.phase))
  )
    return null;
  const result: SelectionSnapshot = {
    operationId: Number(s.operation_id),
    phase: s.phase as SelectionSnapshot["phase"],
  };
  if (s.phase === "ready") {
    if (
      typeof s.text !== "string" ||
      !s.text.length ||
      s.text.length > 32768 ||
      !["ui_automation", "clipboard"].includes(String(s.source))
    )
      return null;
    result.text = s.text;
    result.source = s.source as SelectionSnapshot["source"];
  }
  if (s.phase === "error")
    result.message =
      typeof s.message === "string" && catalog.details.includes(s.message)
        ? s.message
        : "Не удалось безопасно получить выделение. Повторите действие или выберите другое поле.";
  return result;
}
export function createNativeSelectionCapture(
  call: Call,
  subscribe: Subscribe,
  visible: () => boolean = () => true,
): SelectionCapturePort {
  const state = reactive<{ current: SelectionSnapshot | null }>({
    current: null,
  });
  let disposed = false;
  let unlisten: (() => void) | undefined;
  const read = createCoalescedRead(
    () => call<unknown>("get_selection_capture"),
    (value) => {
      state.current = selectionFromNative(value);
    },
    () => !disposed && visible(),
  );
  void subscribe("selection-capture-changed", () => {
    if (!visible()) {
      state.current = null;
      read.invalidate();
      return;
    }
    void read.refresh().catch(() => {});
  })
    .then((release) => {
      if (disposed) release();
      else {
        unlisten = release;
        void read.refresh().catch(() => {});
      }
    })
    .catch(() => {});
  return {
    state,
    refresh: read.refresh,
    async cancel(operationId) {
      if (disposed || state.current?.operationId !== operationId) return;
      await call("cancel_selection_capture", { operationId });
      await read.refresh();
    },
    dispose() {
      disposed = true;
      read.dispose();
      unlisten?.();
      state.current = null;
    },
  };
}
