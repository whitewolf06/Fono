import { useEffect, useState } from "react";
import { ipc, onPipelineStateChange } from "@/lib/ipc";
import type { PipelineState } from "@/lib/types";

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

export function OverlayView() {
  const [state, setState] = useState<PipelineState>("idle");

  useEffect(() => {
    let mounted = true;
    ipc.getPipelineState().then((s) => mounted && setState(s));
    const unlistenP = onPipelineStateChange((s) => setState(s));
    return () => {
      mounted = false;
      unlistenP.then((u) => u());
    };
  }, []);

  const visible = state !== "idle";

  return (
    <div className="flex h-full items-center justify-center">
      {visible && (
        <div className="animate-fade-in pointer-events-none flex items-center gap-3 rounded-full border border-neutral-700/80 bg-neutral-900/90 px-4 py-2 shadow-2xl backdrop-blur-md">
          <span
            className={`h-3 w-3 rounded-full ${
              STATE_COLOR[state]
            } ${state === "listening" ? "animate-pulse-ring" : ""}`}
          />
          <span className="text-sm font-medium text-neutral-100">
            {STATE_LABEL[state]}
          </span>
        </div>
      )}
    </div>
  );
}
