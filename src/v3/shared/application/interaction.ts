import { inject, reactive, type InjectionKey } from "vue";
import type { QuickPanel } from "../domain/contracts";
export interface Confirmation {
  title: string;
  text: string;
  accept: string;
  danger?: boolean;
  cancel?: string;
}
export function createInteraction() {
  const state = reactive<{
    quick: QuickPanel | null;
    confirmation: Confirmation | null;
  }>({ quick: null, confirmation: null });
  let settle: ((value: boolean) => void) | null = null;
  return {
    state,
    openQuick(panel: QuickPanel) {
      state.quick = panel;
    },
    confirm(options: Confirmation): Promise<boolean> {
      settle?.(false);
      state.confirmation = options;
      return new Promise((resolve) => {
        settle = resolve;
      });
    },
    answer(value: boolean) {
      state.confirmation = null;
      settle?.(value);
      settle = null;
    },
  };
}
export const interactionKey: InjectionKey<
  ReturnType<typeof createInteraction>
> = Symbol("fono-interaction");
export function useInteraction() {
  const value = inject(interactionKey);
  if (!value) throw new Error("Interaction is not provided");
  return value;
}
