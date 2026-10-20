import { reactive } from "vue";
import type {
  SelectionCapturePort,
  SelectionSnapshot,
} from "../domain/selection";
export function createMockSelectionCapture(): SelectionCapturePort {
  const state = reactive<{ current: SelectionSnapshot | null }>({
    current: null,
  });
  let next = 0,
    disposed = false;
  let attempted = false;
  function owned(id: number) {
    return !disposed && state.current?.operationId === id
      ? state.current
      : null;
  }
  function begin(id: number, phase: "recording" | "processing") {
    const current = owned(id);
    if (!current || current.edit?.operationId)
      throw new Error("Дождитесь завершения текущего действия с выделением.");
    if (
      current.delivery?.phase === "replacing" ||
      current.delivery?.phase === "copying"
    )
      throw new Error("Дождитесь завершения текущего действия с выделением.");
    current.delivery = { phase: "idle", replaceAllowed: false };
    current.edit = {
      instruction: current.edit?.instruction ?? "",
      result: current.edit?.result,
      resultId: current.edit?.resultId,
      phase,
      operationId: ++next,
    };
    return current;
  }
  return {
    state,
    demo() {
      if (disposed) return;
      attempted = false;
      state.current = {
        operationId: ++next,
        phase: "ready",
        source: "demo",
        text: "Пример выделенного фрагмента. В браузерном демо Fono не читает другие приложения и не использует буфер обмена.",
        edit: { phase: "idle", instruction: "" },
        delivery: { phase: "idle", replaceAllowed: false },
      };
    },
    async cancel(operationId) {
      if (state.current?.operationId === operationId) state.current = null;
    },
    async startInstruction(id) {
      begin(id, "recording");
    },
    async stopInstruction(id, editId) {
      const current = owned(id);
      if (
        current?.edit?.operationId !== editId ||
        current.edit.phase !== "recording"
      )
        return;
      current.edit.phase = "recognizing";
      await Promise.resolve();
      if (owned(id)?.edit?.operationId !== editId) return;
      current.edit = { phase: "idle", instruction: "Сделай формальнее" };
    },
    async cancelEdit(id, editId) {
      const current = owned(id);
      if (current?.edit?.operationId !== editId) return;
      current.edit = {
        phase: "idle",
        instruction: current.edit.instruction,
        result: current.edit.result,
        resultId: current.edit.resultId,
      };
    },
    async transform(id, instruction, transferConfirmed) {
      if (!transferConfirmed)
        throw new Error(
          "Подтвердите отправку выделения и инструкции в выбранное LLM-подключение.",
        );
      if (
        !instruction.trim() ||
        new TextEncoder().encode(instruction).length > 4096 ||
        instruction.includes("\0")
      )
        throw new Error(
          "Введите инструкцию для выделения длиной до 4096 байт.",
        );
      const current = begin(id, "processing");
      const editId = current.edit!.operationId;
      current.edit!.instruction = instruction;
      await Promise.resolve();
      if (owned(id)?.edit?.operationId !== editId) return;
      const text = /переведи/i.test(instruction)
        ? "This is a demonstration of editing the selected text."
        : /сократи/i.test(instruction)
          ? "• Выделенный фрагмент.\n• Пример обработки.\n• Только в памяти."
          : /формаль/i.test(instruction)
            ? "Это демонстрационный результат обработки выделенного фрагмента."
            : current.text;
      current.edit = {
        phase: "ready",
        instruction,
        result: text,
        resultId: editId,
      };
      current.delivery = { phase: "idle", replaceAllowed: !attempted };
    },
    async armReplacement(id, resultId) {
      const current = owned(id);
      if (
        !current?.delivery?.replaceAllowed ||
        current.edit?.resultId !== resultId ||
        attempted ||
        current.edit.operationId
      )
        throw new Error(
          "Повторная замена этого выделения недоступна. Скопируйте результат или получите выделение заново.",
        );
      current.delivery = { phase: "armed", resultId, replaceAllowed: true };
    },
    async cancelDelivery(id, resultId) {
      const current = owned(id);
      if (current?.delivery?.resultId !== resultId) return;
      current.delivery = {
        phase: attempted ? "blocked" : "idle",
        replaceAllowed: !attempted,
      };
    },
    async copyResult(id, resultId) {
      const current = owned(id);
      if (
        !current ||
        current.edit?.resultId !== resultId ||
        current.edit.operationId ||
        ["replacing", "copying", "cancelling"].includes(
          current.delivery?.phase ?? "idle",
        )
      )
        throw new Error(
          "Результат уже изменился. Повторите преобразование выделения.",
        );
      current.delivery = { phase: "copied", replaceAllowed: !attempted };
    },
    async demoReplace() {
      const current = state.current,
        id = current?.operationId,
        resultId = current?.delivery?.resultId;
      if (
        !current ||
        !id ||
        !resultId ||
        current.delivery?.phase !== "armed" ||
        current.edit?.resultId !== resultId ||
        attempted
      )
        throw new Error(
          "Повторная замена этого выделения недоступна. Скопируйте результат или получите выделение заново.",
        );
      attempted = true;
      current.delivery = {
        phase: "replacing",
        resultId,
        replaceAllowed: false,
      };
      await Promise.resolve();
      if (owned(id)?.delivery?.phase !== "replacing") return;
      current.delivery = {
        phase: "submitted",
        resultId,
        replaceAllowed: false,
      };
    },
    dispose() {
      disposed = true;
      state.current = null;
    },
  };
}
