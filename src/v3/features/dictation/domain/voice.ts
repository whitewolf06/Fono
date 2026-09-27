export type VoicePhase =
  "idle" | "listening" | "transcribing" | "processing" | "injecting" | "error";

export interface VoiceEntry {
  id: string;
  text: string;
  createdAt: string;
}

export interface VoiceOverview {
  phase: VoicePhase;
  hotkey: string;
  language: string;
  model: string;
  wakeWordEnabled: boolean;
  wakeWord: string;
  history: VoiceEntry[];
  version: string;
  error: string | null;
}

export const phaseLabels: Record<VoicePhase, string> = {
  idle: "Готов к работе",
  listening: "Слушаю вас",
  transcribing: "Распознаю речь",
  processing: "Обрабатываю текст",
  injecting: "Вставляю текст",
  error: "Нужна проверка",
};

export function formatHistoryDate(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? value
    : new Intl.DateTimeFormat("ru-RU", {
        day: "numeric",
        month: "short",
        hour: "2-digit",
        minute: "2-digit",
      }).format(date);
}
