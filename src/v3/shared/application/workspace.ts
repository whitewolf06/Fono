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
  idle: message("phase.idle"),
  listening: message("phase.listening"),
  silence: message("phase.silence"),
  transcribing: message("phase.transcribing"),
  processing: message("phase.processing"),
  awaiting_action: message("phase.awaiting_action"),
  done: message("phase.done"),
  cancelled: message("phase.cancelled"),
  error: message("phase.error"),
};
