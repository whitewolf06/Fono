import { message as uiMessage } from "../../../shared/domain/messages";
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
    message.value = uiMessage("ui.0e7c50429e");
  }
  async function start() {
    if (capturing.value) return;
    const ticket = ++generation;
    capturing.value = true;
    message.value = uiMessage("ui.2225f79793");
    try {
      const captured = await workspace.triggerCapture.capture((result) => {
        if (ticket !== generation) return;
        session = undefined;
        capturing.value = false;
        if (result.kind === "captured") {
          accept(result.candidate);
          message.value = uiMessage("ui.ee1eb5d008");
        } else
          message.value =
            result.kind === "error"
              ? result.message
              : uiMessage("ui.0e7c50429e");
      });
      if (ticket !== generation) {
        await captured.cancel();
        return;
      }
      session = captured;
      message.value = uiMessage("ui.dc18ff4d8c");
    } catch (error) {
      if (ticket !== generation) return;
      capturing.value = false;
      message.value =
        error instanceof Error ? error.message : uiMessage("ui.a0475c6037");
    }
  }
  onScopeDispose(() => {
    void cancel();
  });
  return { capturing, message, start, cancel };
}
