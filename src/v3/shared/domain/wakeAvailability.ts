// Restore only after the real detector passes the voice reliability checks.
export const WAKE_WORD_AVAILABLE = false;
export const WAKE_WORD_UNAVAILABLE =
  "Временно недоступно · доработаем качество распознавания";
export const WAKE_WORD_ALTERNATIVE =
  "Начинайте диктовку кнопкой или горячей клавишей. Голосовые команды доступны по своей клавише.";

export function availableWakeEnabled(enabled: boolean): boolean {
  return WAKE_WORD_AVAILABLE && enabled;
}

export function ensureWakeAvailable(): void {
  if (!WAKE_WORD_AVAILABLE) throw new Error(WAKE_WORD_UNAVAILABLE);
}
