import type { PipelineState } from "@/lib/types";

const LABEL: Record<PipelineState, string> = {
  idle: "Готов",
  listening: "Слушаю",
  transcribing: "Распознаю",
  processing: "Обрабатываю",
  injecting: "Вставляю",
  error: "Ошибка",
};

const COLOR: Record<PipelineState, string> = {
  idle: "bg-neutral-500",
  listening: "bg-brand-500 animate-pulse-ring",
  transcribing: "bg-amber-500",
  processing: "bg-amber-500",
  injecting: "bg-emerald-500",
  error: "bg-red-600",
};

export function StatusBadge({ state }: { state: PipelineState }) {
  return (
    <div className="inline-flex items-center gap-2 rounded-full border border-neutral-700 bg-neutral-900 px-3 py-1.5 text-xs">
      <span className={`h-2 w-2 rounded-full ${COLOR[state]}`} />
      <span className="font-medium text-neutral-200">{LABEL[state]}</span>
    </div>
  );
}
