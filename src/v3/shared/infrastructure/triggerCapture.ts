import type { WorkspaceState } from "../domain/contracts";
import type {
  TriggerCandidate,
  TriggerCapturePort,
  TriggerCaptureResult,
} from "../domain/triggers";
import { shortcutParts } from "../domain/triggers";

// Focused-window capture only. No global listener, text, event log or persistence.
export function createTriggerCapture(
  state: WorkspaceState,
  native?: { begin(): Promise<string>; end(id: string): Promise<void> },
): TriggerCapturePort {
  return {
    async capture(report) {
      if (
        state.triggerCaptureActive ||
        [
          "listening",
          "silence",
          "transcribing",
          "processing",
          "awaiting_action",
        ].includes(state.phase)
      )
        throw new Error("Сначала завершите текущую диктовку или выбор кнопки.");
      state.triggerCaptureActive = true;
      let id: string | undefined;
      try {
        id = await native?.begin();
      } catch (error) {
        state.triggerCaptureActive = false;
        throw error;
      }
      if (globalThis.document?.hasFocus?.() === false) {
        try {
          if (id) await native?.end(id);
        } finally {
          state.triggerCaptureActive = false;
        }
        throw new Error("Выбор отменён: окно потеряло фокус.");
      }
      let pending: { code: string; candidate: TriggerCandidate } | undefined;
      let ending: Promise<void> | undefined;
      let mouseDown: number | undefined;
      const finish = (result: TriggerCaptureResult): Promise<void> => {
        if (ending) return ending;
        remove();
        pending = undefined;
        mouseDown = undefined;
        ending = (async () => {
          try {
            if (id) await native?.end(id);
          } catch {
            result = {
              kind: "error",
              message:
                "Не удалось восстановить горячие клавиши после выбора. Сохраните настройки заново или перезапустите Fono.",
            };
          } finally {
            state.triggerCaptureActive = false;
          }
          report(result);
        })();
        return ending;
      };
      const stop = (event: Event) => {
        event.preventDefault();
        event.stopImmediatePropagation();
      };
      const keydown = (event: KeyboardEvent) => {
        stop(event);
        if (event.repeat) return;
        if (event.code === "Escape") {
          void finish({ kind: "cancelled" });
          return;
        }
        const single = {
          AltRight: "right_alt",
          ControlRight: "right_ctrl",
          CapsLock: "caps_lock",
        } as const;
        const trigger = single[event.code as keyof typeof single];
        if (trigger) {
          if (!pending) pending = { code: event.code, candidate: { trigger } };
          return;
        }
        if (
          ["ControlLeft", "AltLeft", "ShiftLeft", "ShiftRight"].includes(
            event.code,
          )
        )
          return;
        const key = /^Key[A-Z]$/.test(event.code)
          ? event.code.slice(3)
          : /^Digit[0-9]$/.test(event.code)
            ? event.code.slice(5)
            : event.code;
        const parts = shortcutParts(
          [
            ...(event.ctrlKey ? ["Ctrl"] : []),
            ...(event.altKey ? ["Alt"] : []),
            ...(event.shiftKey ? ["Shift"] : []),
            key,
          ].join(" + "),
        );
        if (!parts || event.metaKey) {
          void finish({
            kind: "error",
            message:
              "Поддерживаются правые Alt/Ctrl, CapsLock, боковые кнопки мыши и сочетания Ctrl/Alt/Shift с буквой, цифрой, Space или F1–F12.",
          });
          return;
        }
        pending = {
          code: event.code,
          candidate: { trigger: "shortcut", hotkey: parts.join(" + ") },
        };
      };
      const keyup = (event: KeyboardEvent) => {
        stop(event);
        if (pending?.code === event.code)
          void finish({ kind: "captured", candidate: pending.candidate });
      };
      const mouse = (event: MouseEvent) => {
        if (event.button !== 3 && event.button !== 4) return;
        stop(event);
        if (event.type === "mousedown") mouseDown = event.button;
        if (event.type === "mouseup" && mouseDown === event.button)
          void finish({
            kind: "captured",
            candidate: {
              trigger: event.button === 3 ? "x_button1" : "x_button2",
            },
          });
      };
      const blur = () => {
        void finish({ kind: "cancelled" });
      };
      const timer = setTimeout(blur, 30_000);
      function remove() {
        clearTimeout(timer);
        window.removeEventListener("keydown", keydown, true);
        window.removeEventListener("keyup", keyup, true);
        window.removeEventListener("mousedown", mouse, true);
        window.removeEventListener("mouseup", mouse, true);
        window.removeEventListener("auxclick", stop, true);
        window.removeEventListener("blur", blur);
      }
      window.addEventListener("keydown", keydown, true);
      window.addEventListener("keyup", keyup, true);
      window.addEventListener("mousedown", mouse, true);
      window.addEventListener("mouseup", mouse, true);
      window.addEventListener("auxclick", stop, true);
      window.addEventListener("blur", blur);
      return { cancel: () => finish({ kind: "cancelled" }) };
    },
  };
}
