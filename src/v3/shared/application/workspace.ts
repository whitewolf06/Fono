import { inject, type InjectionKey } from "vue";
import type { Workspace } from "../domain/contracts";
export const workspaceKey: InjectionKey<Workspace> = Symbol("fono-workspace");
export function useWorkspace(): Workspace {
  const workspace = inject(workspaceKey);
  if (!workspace) throw new Error("Fono workspace is not provided");
  return workspace;
}
export const phaseLabels = {
  idle: "Готов слушать",
  listening: "Слушаю вас",
  silence: "Жду продолжения",
  transcribing: "Распознаю речь",
  processing: "Привожу текст в порядок",
  done: "Текст готов",
  cancelled: "Запись отменена",
  error: "Нужна ваша помощь",
};
