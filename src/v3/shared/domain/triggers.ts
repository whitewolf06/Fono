import { message as uiMessage } from "./messages";
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
        get right_alt() {
          return uiMessage("ui.3424783930");
        },
        get right_ctrl() {
          return uiMessage("ui.671c2fb9e5");
        },
        caps_lock: "CapsLock",
        get x_button1() {
          return uiMessage("ui.b1039f55bd");
        },
        get x_button2() {
          return uiMessage("ui.8fbdc02568");
        },
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
  selectionCaptureEnabled?: boolean;
  selectionHotkey?: string;
}): string | null {
  if (!dictationTriggers.includes(p.dictationTrigger))
    return uiMessage("ui.de849c8613");
  const command = shortcutParts(p.commandHotkey);
  if (!command) return uiMessage("ui.2e0e3829fc");
  if (command.join("+") === "Alt+F4") return uiMessage("ui.5a1a70cfc0");
  if (p.selectionCaptureEnabled) {
    const selection = shortcutParts(p.selectionHotkey || "");
    if (!selection) return uiMessage("ui.ff7d801186");
    if (selection.join("+") === "Alt+F4") return uiMessage("ui.5a1a70cfc0");
    if (
      selection.join("+") === command.join("+") ||
      (p.dictationTrigger === "shortcut" &&
        selection.join("+") === shortcutParts(p.hotkey)?.join("+")) ||
      (p.dictationTrigger === "right_ctrl" && selection.includes("Ctrl")) ||
      (p.dictationTrigger === "right_alt" && selection.includes("Alt"))
    )
      return uiMessage("ui.95934e52cc");
  }
  if (p.dictationTrigger === "shortcut") {
    const dictation = shortcutParts(p.hotkey);
    if (!dictation) return uiMessage("ui.93965741b1");
    if (dictation.join("+") === "Alt+F4") return uiMessage("ui.5a1a70cfc0");
    if (dictation.join("+") === command.join("+"))
      return uiMessage("ui.aeab8bec93");
  }
  if (
    (p.dictationTrigger === "right_ctrl" && command.includes("Ctrl")) ||
    (p.dictationTrigger === "right_alt" && command.includes("Alt"))
  )
    return uiMessage("ui.ed5b729af1");
  return null;
}
