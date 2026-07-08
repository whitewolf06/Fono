import { useEffect, useRef, useState } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { ipc, onPipelineStateChange } from "@/lib/ipc";
import type { PipelineState, Settings } from "@/lib/types";

const STATE_LABEL: Record<PipelineState, string> = {
  idle: "Готов",
  listening: "Слушаю…",
  transcribing: "Распознаю…",
  processing: "Обрабатываю…",
  injecting: "Вставляю…",
  error: "Ошибка",
};

const STATE_COLOR: Record<PipelineState, string> = {
  idle: "bg-neutral-500",
  listening: "bg-brand-500",
  transcribing: "bg-amber-500",
  processing: "bg-amber-500",
  injecting: "bg-emerald-500",
  error: "bg-red-600",
};

const BASE_WIDTH = 160;
const BASE_HEIGHT = 64;

export function OverlayView() {
  const [state, setState] = useState<PipelineState>("idle");
  const [settings, setSettings] = useState<Settings>({
    overlay_scale: 1,
    overlay_opacity: 1,
    overlay_mini_mode: false,
  } as Settings);
  const saveTimer = useRef<number | null>(null);

  useEffect(() => {
    let mounted = true;
    ipc.getPipelineState().then((s) => mounted && setState(s));
    ipc.getSettings().then((s) => mounted && setSettings(s));
    const unlistenP = onPipelineStateChange((s) => setState(s));
    return () => {
      mounted = false;
      unlistenP.then((u) => u());
    };
  }, []);

  useEffect(() => {
    const win = getCurrentWebviewWindow();
    const width = Math.round(BASE_WIDTH * settings.overlay_scale);
    const height = Math.round(BASE_HEIGHT * settings.overlay_scale);
    win.setSize(new LogicalSize(width, height)).catch(() => {});
  }, [settings.overlay_scale]);

  useEffect(() => {
    const win = getCurrentWebviewWindow();
    let unlisten: (() => void) | undefined;

    const setup = async () => {
      unlisten = await win.onMoved(({ payload: { x, y } }) => {
        if (saveTimer.current) {
          window.clearTimeout(saveTimer.current);
        }
        saveTimer.current = window.setTimeout(() => {
          ipc.saveOverlayPosition(x, y).catch(() => {});
        }, 500);
      });
    };
    setup();

    return () => {
      if (unlisten) unlisten();
      if (saveTimer.current) window.clearTimeout(saveTimer.current);
    };
  }, []);

  const handleMouseDown = () => {
    const win = getCurrentWebviewWindow();
    win.startDragging().catch(() => {});
  };

  const visible = state !== "idle";

  return (
    <div
      className="flex h-full select-none items-center justify-center"
      onMouseDown={handleMouseDown}
      title="Перетащи меня мышью"
      style={{ opacity: settings.overlay_opacity }}
    >
      {visible && (
        <div
          className="animate-fade-in flex cursor-move items-center gap-3 rounded-full border border-neutral-700/80 bg-neutral-900/90 px-4 py-2 shadow-2xl backdrop-blur-md"
          style={{ transform: `scale(${settings.overlay_scale})` }}
        >
          <span
            className={`h-3 w-3 rounded-full ${
              STATE_COLOR[state]
            } ${state === "listening" ? "animate-pulse-ring" : ""}`}
          />
          {!settings.overlay_mini_mode && (
            <span className="text-sm font-medium text-neutral-100">
              {STATE_LABEL[state]}
            </span>
          )}
        </div>
      )}
    </div>
  );
}
