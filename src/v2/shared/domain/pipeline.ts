export type DictationPhase =
  | "idle"
  | "listening"
  | "transcribing"
  | "processing"
  | "injecting"
  | "error";

export interface DictationSnapshot {
  phase: DictationPhase;
  mode: "dictation" | "command";
  transcript: string;
  hotkey: string;
  language: string;
  error: string | null;
}

export interface ReadinessSnapshot {
  microphone: "ready" | "attention";
  model: "ready" | "missing";
  wakeWord: "active" | "paused" | "disabled";
}

export const phaseCopy: Record<DictationPhase, { label: string; hint: string }> = {
  idle: { label: "Готово", hint: "Нажмите кнопку или используйте горячую клавишу." },
  listening: { label: "Слушаю", hint: "Говорите естественно — Fono позаботится об остальном." },
  transcribing: { label: "Распознаю", hint: "Преобразую аудио в текст локально." },
  processing: { label: "Обрабатываю", hint: "Подготавливаю текст к вставке." },
  injecting: { label: "Вставляю", hint: "Отправляю готовый текст в активное окно." },
  error: { label: "Нужна проверка", hint: "Откройте диагностику и проверьте подключение." },
};
