import { message } from "../domain/messages";
import { inject, type InjectionKey } from "vue";
import type { Workspace } from "../domain/contracts";
export const workspaceKey: InjectionKey<Workspace> = Symbol("fono-workspace");
export function useWorkspace(): Workspace {
  const workspace = inject(workspaceKey);
  if (!workspace) throw new Error("Fono workspace is not provided");
  return workspace;
}
export const phaseLabels = {
  get idle() {
    return message("phase.idle");
  },
  get listening() {
    return message("phase.listening");
  },
  get silence() {
    return message("phase.silence");
  },
  get transcribing() {
    return message("phase.transcribing");
  },
  get processing() {
    return message("phase.processing");
  },
  get awaiting_action() {
    return message("phase.awaiting_action");
  },
  get done() {
    return message("phase.done");
  },
  get cancelled() {
    return message("phase.cancelled");
  },
  get error() {
    return message("phase.error");
  },
};
