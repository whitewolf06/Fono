import { onScopeDispose, ref } from "vue";
import { useWorkspace } from "../../../shared/application/workspace";
import type { TriggerCandidate } from "../../../shared/domain/triggers";

export function useTriggerCapture(
  accept: (candidate: TriggerCandidate) => void,
) {
  const workspace = useWorkspace();
  const capturing = ref(false);
  const message = ref("");
  let generation = 0;
  let session: { cancel(): Promise<void> } | undefined;
  async function cancel() {
    generation++;
    const current = session;
    session = undefined;
    if (current) await current.cancel();
    capturing.value = false;
    message.value = "Выбор отменён. Прежняя кнопка сохранена.";
  }
  async function start() {
    if (capturing.value) return;
    const ticket = ++generation;
    capturing.value = true;
    message.value = "Подготовка выбора кнопки…";
    try {
      const captured = await workspace.triggerCapture.capture((result) => {
        if (ticket !== generation) return;
        session = undefined;
        capturing.value = false;
        if (result.kind === "captured") {
          accept(result.candidate);
          message.value =
            "Кнопка выбрана. Нажмите «Сохранить», чтобы применить.";
        } else
          message.value =
            result.kind === "error"
              ? result.message
              : "Выбор отменён. Прежняя кнопка сохранена.";
      });
      if (ticket !== generation) {
        await captured.cancel();
        return;
      }
      session = captured;
      message.value =
        "Нажмите и отпустите кнопку или сочетание. Esc — отмена. Выбор действует 30 секунд, пока окно активно.";
    } catch (error) {
      if (ticket !== generation) return;
      capturing.value = false;
      message.value =
        error instanceof Error
          ? error.message
          : "Не удалось начать выбор кнопки.";
    }
  }
  onScopeDispose(() => {
    void cancel();
  });
  return { capturing, message, start, cancel };
}
