import { message as uiMessage } from "./messages";
// Restore only after the real detector passes the voice reliability checks.
export const WAKE_WORD_AVAILABLE = false;
export const WAKE_WORD_UNAVAILABLE = uiMessage("ui.dea58178d5");
export const WAKE_WORD_ALTERNATIVE = uiMessage("ui.ced052973a");
export function unavailableWakeMessage(): string {
  return uiMessage("ui.dea58178d5");
}
export function alternativeWakeMessage(): string {
  return uiMessage("ui.ced052973a");
}

export function availableWakeEnabled(enabled: boolean): boolean {
  return WAKE_WORD_AVAILABLE && enabled;
}

export function ensureWakeAvailable(): void {
  if (!WAKE_WORD_AVAILABLE) throw new Error(unavailableWakeMessage());
}
