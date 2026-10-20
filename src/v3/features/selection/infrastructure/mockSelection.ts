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
  return {
    state,
    demo() {
      if (disposed) return;
      state.current = {
        operationId: ++next,
        phase: "ready",
        source: "demo",
        text: "Пример выделенного фрагмента. В браузерном демо Fono не читает другие приложения и не использует буфер обмена.",
      };
    },
    async cancel(operationId) {
      if (state.current?.operationId === operationId) state.current = null;
    },
    dispose() {
      disposed = true;
      state.current = null;
    },
  };
}
