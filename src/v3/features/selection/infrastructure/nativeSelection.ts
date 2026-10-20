import { reactive } from "vue";
import type {
  SelectionCapturePort,
  SelectionSnapshot,
  SelectionEditSnapshot,
  SelectionDeliverySnapshot,
} from "../domain/selection";
import { createCoalescedRead } from "../../../shared/infrastructure/native/coalescedRead";
import catalog from "../../../shared/infrastructure/native/userErrorCatalog.json";
type Call = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
type Subscribe = (
  channel: string,
  handler: (value: unknown) => void,
) => Promise<() => void>;

function editFromNative(value: unknown): SelectionEditSnapshot | undefined {
  if (!value || typeof value !== "object") return;
  const e = value as Record<string, unknown>;
  if (
    ![
      "idle",
      "starting",
      "recording",
      "recognizing",
      "processing",
      "cancelling",
      "ready",
    ].includes(String(e.phase))
  )
    return;
  const edit: SelectionEditSnapshot = {
    phase: e.phase as SelectionEditSnapshot["phase"],
    instruction:
      typeof e.instruction === "string" && e.instruction.length <= 4096
        ? e.instruction
        : "",
  };
  if (Number.isSafeInteger(e.operation_id) && Number(e.operation_id) > 0)
    edit.operationId = Number(e.operation_id);
  if (typeof e.result === "string" && e.result.length <= 32768)
    edit.result = e.result;
  if (Number.isSafeInteger(e.result_id) && Number(e.result_id) > 0)
    edit.resultId = Number(e.result_id);
  if (typeof e.message === "string")
    edit.message = catalog.details.includes(e.message)
      ? e.message
      : "Не удалось преобразовать выделение. Оригинал сохранён; повторите действие.";
  return edit;
}

function deliveryFromNative(
  value: unknown,
): SelectionDeliverySnapshot | undefined {
  if (!value || typeof value !== "object") return;
  const d = value as Record<string, unknown>;
  if (
    ![
      "idle",
      "armed",
      "replacing",
      "copying",
      "cancelling",
      "submitted",
      "blocked",
      "uncertain",
      "copied",
    ].includes(String(d.phase))
  )
    return;
  const result: SelectionDeliverySnapshot = {
    phase: d.phase as SelectionDeliverySnapshot["phase"],
    replaceAllowed: d.replace_allowed === true,
  };
  if (Number.isSafeInteger(d.operation_id) && Number(d.operation_id) > 0)
    result.operationId = Number(d.operation_id);
  if (Number.isSafeInteger(d.result_id) && Number(d.result_id) > 0)
    result.resultId = Number(d.result_id);
  if (typeof d.message === "string")
    result.message = catalog.details.includes(d.message)
      ? d.message
      : "Действие не подтверждено. Результат доступен для копирования; повторная вставка заблокирована.";
  return result;
}

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
  const edit = editFromNative(s.edit);
  if (edit) result.edit = edit;
  const delivery = deliveryFromNative(s.delivery);
  if (delivery) result.delivery = delivery;
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
  async function action(
    operationId: number,
    command: string,
    args: Record<string, unknown> = {},
  ) {
    if (disposed || state.current?.operationId !== operationId) return;
    await call(command, { operationId, ...args });
    await read.refresh();
  }
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
    armReplacement: (id, resultId) =>
      action(id, "arm_selection_replace", { resultId }),
    cancelDelivery: (id, resultId) =>
      action(id, "cancel_selection_delivery", { resultId }),
    copyResult: (id, resultId) =>
      action(id, "copy_selection_result", { resultId }),
    startInstruction: (id) => action(id, "start_selection_instruction"),
    stopInstruction: (id, editOperationId) =>
      action(id, "stop_selection_instruction", { editOperationId }),
    cancelEdit: (id, editOperationId) =>
      action(id, "cancel_selection_edit", { editOperationId }),
    transform: (id, instruction, transferConfirmed) =>
      action(id, "transform_selection", {
        input: { instruction, transferConfirmed },
      }),
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
