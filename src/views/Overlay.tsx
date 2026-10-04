import { useEffect, useRef, useState } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { LogicalSize } from "@tauri-apps/api/dpi";
import {
  ipc,
  onPipelineMode,
  onPipelineStateChange,
  onSettingsChange,
  onWakeDictationCountdown,
} from "@/lib/ipc";
import type {
  PipelineMode,
  PipelineState,
  Settings,
  WakeDictationCountdown,
} from "@/lib/types";

const STATE_LABEL: Record<PipelineState, string> = {
  idle: "Готов",
  listening: "Слушаю…",
  transcribing: "Распознаю…",
  processing: "Обрабатываю…",
  awaiting_action: "Выберите действие в Fono V3",
  injecting: "Вставляю…",
  error: "Ошибка",
};

const STATE_COLOR: Record<PipelineState, string> = {
  idle: "bg-neutral-500",
  listening: "bg-brand-500",
  transcribing: "bg-amber-500",
  processing: "bg-amber-500",
  awaiting_action: "bg-amber-500",
  injecting: "bg-emerald-500",
  error: "bg-red-600",
};

// Таймер и две кнопки не помещаются в старые 200 px: flex-элементы выходили
// за границы webview, из-за чего оверлей выглядел зависшим/обрезанным.
const BASE_WIDTH = 286;
const BASE_HEIGHT = 88;

export function OverlayView() {
  const [state, setState] = useState<PipelineState>("idle");
  const [mode, setMode] = useState<PipelineMode>("dictation");
  const [wakeCountdown, setWakeCountdown] =
    useState<WakeDictationCountdown | null>(null);
  const [settings, setSettings] = useState<Settings>({
    overlay_scale: 1,
    overlay_opacity: 1,
    overlay_mini_mode: false,
  } as Settings);
  const saveTimer = useRef<number | null>(null);
  const pipelineStateRef = useRef<PipelineState>("idle");

  useEffect(() => {
    let mounted = true;
    ipc.getPipelineState().then((s) => {
      if (!mounted) return;
      pipelineStateRef.current = s;
      setState(s);
    });
    ipc.getSettings().then((s) => mounted && setSettings(s));
    const unlistenP = onPipelineStateChange((s) => {
      pipelineStateRef.current = s;
      setState(s);
      if (s !== "listening") setWakeCountdown(null);
    });
    const unlistenM = onPipelineMode((m) => mounted && setMode(m));
    const unlistenS = onSettingsChange((s) => mounted && setSettings(s));
    const unlistenC = onWakeDictationCountdown((countdown) => {
      // Событие может физически прийти уже после перехода к распознаванию.
      // Не сохраняем такой устаревший таймер до следующей записи.
      if (mounted && pipelineStateRef.current === "listening") {
        setWakeCountdown(countdown);
      }
    });
    return () => {
      mounted = false;
      unlistenP.then((u) => u());
      unlistenM.then((u) => u());
      unlistenS.then((u) => u());
      unlistenC.then((u) => u());
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

  const handleStop = (e: React.MouseEvent) => {
    e.stopPropagation();
    ipc.cancelDictation().catch(() => {});
  };

  const handleConfirm = (e: React.MouseEvent) => {
    e.stopPropagation();
    ipc.confirmDictation().catch(() => {});
  };

  const visible = state !== "idle";
  const modeLabel = mode === "command" ? "команда" : "диктовка";
  const hasWakeCountdown = state === "listening" && wakeCountdown !== null;
  const countdownSeconds = wakeCountdown
    ? (wakeCountdown.remaining_ms / 1000).toFixed(1)
    : "0.0";
  const countdownPercent = wakeCountdown
    ? Math.max(
        0,
        Math.min(
          100,
          wakeCountdown.timeout_ms > 0
            ? (wakeCountdown.remaining_ms / wakeCountdown.timeout_ms) * 100
            : 0,
        ),
      )
    : 0;

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
            <div className="flex flex-col">
              <span className="text-sm font-medium text-neutral-100">
                {STATE_LABEL[state]}
              </span>
              <span className="text-[10px] uppercase tracking-wider text-neutral-400">
                {modeLabel}
              </span>
              {hasWakeCountdown && (
                <div className="mt-1 w-28">
                  <div className="flex justify-between text-[10px] text-neutral-300">
                    <span>
                      {wakeCountdown.speaking
                        ? "Пауза до перевода"
                        : "Перевод через"}
                    </span>
                    <span>{countdownSeconds} с</span>
                  </div>
                  <div className="mt-0.5 h-1 overflow-hidden rounded-full bg-neutral-700">
                    <div
                      className="h-full rounded-full bg-brand-400 transition-[width] duration-100"
                      style={{ width: `${countdownPercent}%` }}
                    />
                  </div>
                </div>
              )}
            </div>
          )}
          {state === "listening" && (
            <button
              type="button"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={handleConfirm}
              className="ml-1 flex h-6 w-6 cursor-pointer items-center justify-center rounded-full bg-emerald-600/80 text-[10px] font-bold text-white hover:bg-emerald-500"
              title="Подтвердить (закончить запись)"
            >
              ✓
            </button>
          )}
          <button
            type="button"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={handleStop}
            className="ml-1 flex h-6 w-6 cursor-pointer items-center justify-center rounded-full bg-red-600/80 text-[10px] font-bold text-white hover:bg-red-500"
            title="Отменить"
          >
            ■
          </button>
        </div>
      )}
    </div>
  );
}
