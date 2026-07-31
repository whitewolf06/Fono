import { useState, type KeyboardEvent } from "react";

interface ShortcutRecorderProps {
  label: string;
  value: string;
  defaultValue: string;
  conflicts?: Array<{ value: string; label: string }>;
  disabled?: boolean;
  onChange: (value: string) => void;
}

export function ShortcutRecorder({
  label,
  value,
  defaultValue,
  conflicts = [],
  disabled = false,
  onChange,
}: ShortcutRecorderProps) {
  const [isRecording, setIsRecording] = useState(false);
  const [hint, setHint] = useState("Нажмите сочетание — Esc отменит запись.");
  const [error, setError] = useState<string | null>(null);

  const reset = () => {
    setError(null);
    setHint("Восстановлено рекомендуемое сочетание.");
    onChange(defaultValue);
  };

  const recordShortcut = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (!isRecording) return;

    event.preventDefault();

    if (event.key === "Escape") {
      setIsRecording(false);
      setError(null);
      setHint("Запись отменена.");
      return;
    }

    if (["Control", "Alt", "Shift", "Meta"].includes(event.key)) {
      setHint("Добавьте основную клавишу к сочетанию.");
      return;
    }

    const shortcut = formatShortcut(event);

    if (!hasModifier(event)) {
      setError("Добавьте Ctrl, Alt или Shift — одиночная клавиша небезопасна.");
      return;
    }

    const conflict = conflicts.find(
      (item) => normalizeShortcut(item.value) === normalizeShortcut(shortcut),
    );

    if (conflict) {
      setError(
        `Конфликт: ${conflict.label} уже использует «${conflict.value}».`,
      );
      return;
    }

    setIsRecording(false);
    setError(null);
    setHint("Сочетание записано. Оно будет применено после сохранения.");
    onChange(shortcut);
  };

  return (
    <div className={`v2-shortcut-recorder ${error ? "has-error" : ""}`}>
      <span>{label}</span>
      <div className="v2-shortcut-recorder__controls">
        <button
          type="button"
          disabled={disabled}
          aria-pressed={isRecording}
          onClick={() => {
            setIsRecording(true);
            setError(null);
            setHint("Ожидаю сочетание клавиш…");
          }}
          onKeyDown={recordShortcut}
        >
          {isRecording ? "Нажмите сочетание…" : value}
        </button>
        <button
          className="v2-button v2-button--ghost"
          type="button"
          disabled={disabled || value === defaultValue}
          onClick={reset}
        >
          Сбросить
        </button>
      </div>
      <small>{error ?? hint}</small>
    </div>
  );
}

function hasModifier(event: KeyboardEvent<HTMLButtonElement>) {
  return event.ctrlKey || event.altKey || event.shiftKey || event.metaKey;
}

function formatShortcut(event: KeyboardEvent<HTMLButtonElement>) {
  const parts = [
    event.ctrlKey && "Ctrl",
    event.altKey && "Alt",
    event.shiftKey && "Shift",
    event.metaKey && "Win",
    formatKey(event.key),
  ].filter(Boolean);

  return parts.join(" + ");
}

function formatKey(key: string) {
  if (key === " ") return "Space";
  if (key.length === 1) return key.toUpperCase();
  return key;
}

function normalizeShortcut(shortcut: string) {
  return shortcut.replaceAll(" ", "").toLocaleLowerCase();
}
