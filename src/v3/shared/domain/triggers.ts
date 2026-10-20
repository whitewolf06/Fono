export const dictationTriggers = [
  "shortcut",
  "right_alt",
  "right_ctrl",
  "caps_lock",
  "x_button1",
  "x_button2",
] as const;
export type DictationTrigger = (typeof dictationTriggers)[number];
export function triggerLabel(p: {
  dictationTrigger: DictationTrigger;
  hotkey: string;
}): string {
  return p.dictationTrigger === "shortcut"
    ? p.hotkey
    : {
        right_alt: "Правый Alt",
        right_ctrl: "Правый Ctrl",
        caps_lock: "CapsLock",
        x_button1: "Кнопка мыши 1",
        x_button2: "Кнопка мыши 2",
      }[p.dictationTrigger];
}
export type TriggerCandidate = { trigger: DictationTrigger; hotkey?: string };
export type TriggerCaptureResult =
  | { kind: "captured"; candidate: TriggerCandidate }
  | { kind: "cancelled" }
  | { kind: "error"; message: string };
export interface TriggerCapturePort {
  capture(
    report: (result: TriggerCaptureResult) => void,
  ): Promise<{ cancel(): Promise<void> }>;
}
export function availableTrigger(value: unknown): DictationTrigger {
  return dictationTriggers.includes(value as DictationTrigger)
    ? (value as DictationTrigger)
    : "shortcut";
}
export function shortcutParts(value: string): string[] | null {
  const parts = value.split("+").map((p) => p.trim());
  const key = parts.pop();
  if (
    !key ||
    !/^(?:[A-Z0-9]|Space|F[1-9]|F1[0-2])$/.test(key) ||
    !parts.length ||
    parts.some((p) => !["Ctrl", "Alt", "Shift"].includes(p)) ||
    new Set(parts).size !== parts.length
  )
    return null;
  return [...["Ctrl", "Alt", "Shift"].filter((p) => parts.includes(p)), key];
}
export function triggerConflict(p: {
  dictationTrigger: DictationTrigger;
  hotkey: string;
  commandHotkey: string;
}): string | null {
  if (!dictationTriggers.includes(p.dictationTrigger))
    return "Выберите кнопку диктовки.";
  const command = shortcutParts(p.commandHotkey);
  if (!command)
    return "Укажите сочетание для команд, например Ctrl + Shift + Space.";
  if (command.join("+") === "Alt+F4")
    return "Alt + F4 закрывает окно. Выберите другое сочетание.";
  if (p.dictationTrigger === "shortcut") {
    const dictation = shortcutParts(p.hotkey);
    if (!dictation)
      return "Укажите сочетание, например Ctrl + Space или Alt + F9.";
    if (dictation.join("+") === "Alt+F4")
      return "Alt + F4 закрывает окно. Выберите другое сочетание.";
    if (dictation.join("+") === command.join("+"))
      return "Клавиши диктовки и команд должны отличаться.";
  }
  if (
    (p.dictationTrigger === "right_ctrl" && command.includes("Ctrl")) ||
    (p.dictationTrigger === "right_alt" && command.includes("Alt"))
  )
    return "Кнопка диктовки входит в сочетание команд. Измените сочетание команд или выберите другую кнопку.";
  return null;
}
