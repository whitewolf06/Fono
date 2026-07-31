import { useEffect, useState } from "react";

interface TranscriptResultCardProps {
  transcript: string;
  isProcessing: boolean;
  onOpenHistory: () => void;
}

type TranscriptAction =
  | "edit"
  | "copy"
  | "reinsert"
  | "restore"
  | "history"
  | "clear";

const actionCopy: Record<TranscriptAction, string> = {
  edit: "Редактировать",
  copy: "Копировать",
  reinsert: "Вставить повторно",
  restore: "Вернуть исходный",
  history: "История",
  clear: "Очистить",
};

export function TranscriptResultCard({
  transcript,
  isProcessing,
  onOpenHistory,
}: TranscriptResultCardProps) {
  const hasTranscript = transcript.trim().length > 0;
  const [editableText, setEditableText] = useState(transcript);
  const [isEditing, setIsEditing] = useState(false);

  useEffect(() => {
    setEditableText(transcript);
    setIsEditing(false);
  }, [transcript]);
  const status = isProcessing
    ? "Обрабатываю…"
    : hasTranscript
      ? "Готово"
      : "Ожидание";

  return (
    <section
      className={`v2-transcript-result-card ${
        hasTranscript ? "has-result" : ""
      } ${isProcessing ? "is-processing" : ""}`}
      aria-live="polite"
    >
      <header className="v2-transcript-result-card__header">
        <h2>{hasTranscript ? "Последняя диктовка" : "Текст диктовки"}</h2>
        <span
          className={`v2-transcript-result-card__status ${
            isProcessing ? "is-processing" : hasTranscript ? "is-ready" : ""
          }`}
        >
          {status}
          <StatusIcon
            state={
              isProcessing ? "processing" : hasTranscript ? "ready" : "idle"
            }
          />
        </span>
      </header>

      <div className="v2-transcript-result-card__body">
        {isEditing ? (
          <textarea
            aria-label="Текст диктовки"
            value={editableText}
            onChange={(event) => setEditableText(event.target.value)}
          />
        ) : (
          <p>
            {hasTranscript
              ? editableText
              : "Здесь появится текст после завершения диктовки."}
          </p>
        )}
        {hasTranscript && (
          <footer className="v2-transcript-result-card__metadata">
            <MetadataIcon />
            <span>
              Вставлено в <strong>Visual Studio Code</strong>
            </span>
            <i aria-hidden="true" />
            <time>12 сек назад</time>
          </footer>
        )}
      </div>

      <div
        className="v2-transcript-result-card__actions"
        role="group"
        aria-label="Действия с диктовкой"
      >
        <TranscriptActionButton
          action="edit"
          disabled={!hasTranscript}
          onClick={() => setIsEditing((value) => !value)}
        />
        <TranscriptActionButton action="copy" disabled={!hasTranscript} />
        <TranscriptActionButton action="reinsert" disabled={!hasTranscript} />
        <TranscriptActionButton action="restore" disabled={!hasTranscript} />
        <span
          className="v2-transcript-result-card__divider"
          aria-hidden="true"
        />
        <TranscriptActionButton action="history" onClick={onOpenHistory} />
        <TranscriptActionButton action="clear" disabled={!hasTranscript} />
      </div>
    </section>
  );
}

function TranscriptActionButton({
  action,
  disabled = false,
  onClick,
}: {
  action: TranscriptAction;
  disabled?: boolean;
  onClick?: () => void;
}) {
  return (
    <button
      className={`v2-transcript-result-card__action v2-transcript-result-card__action--${action}`}
      type="button"
      disabled={disabled}
      onClick={onClick}
      title={
        disabled ? "Появится после завершения диктовки" : actionCopy[action]
      }
    >
      <ActionIcon action={action} />
      <span>{actionCopy[action]}</span>
    </button>
  );
}

function StatusIcon({ state }: { state: "processing" | "ready" | "idle" }) {
  if (state === "processing") {
    return (
      <span
        className="v2-transcript-result-card__spinner"
        aria-label="Выполняется обработка"
      />
    );
  }

  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      {state === "ready" ? (
        <path d="m7.4 12.5 3 3 6.3-7" />
      ) : (
        <path d="M12 7.5v5l3 1.8" />
      )}
      <circle cx="12" cy="12" r="8.5" />
    </svg>
  );
}

function MetadataIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <rect x="4" y="5" width="16" height="14" rx="2.5" />
      <path d="M8 3v4M16 3v4M7.5 11h9M7.5 15h5" />
    </svg>
  );
}

function ActionIcon({ action }: { action: TranscriptAction }) {
  const paths: Record<TranscriptAction, JSX.Element> = {
    edit: (
      <>
        <path d="m5 19 2.8-.7L18.2 7.9 16.1 5.8 5.7 16.2 5 19Z" />
        <path d="m14.9 7 2.1 2.1" />
      </>
    ),
    copy: (
      <>
        <rect x="8" y="8" width="10" height="11" rx="2" />
        <path d="M6 15H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v1" />
      </>
    ),
    reinsert: (
      <>
        <rect x="7" y="5" width="11" height="13" rx="2" />
        <path d="M5 9H4a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h9a2 2 0 0 0 2-2v-1M11 11h9M16 6l4 5-4 5" />
      </>
    ),
    restore: (
      <>
        <path d="M8.2 7.2H4.5V3.5M4.7 7.2A8 8 0 1 1 4 14" />
        <path d="M8 12h5" />
      </>
    ),
    history: (
      <>
        <circle cx="12" cy="12" r="8.5" />
        <path d="M12 7v5l3.2 2" />
      </>
    ),
    clear: (
      <>
        <path d="M5 7h14M9 7V5h6v2M7.5 7l.7 12h7.6l.7-12M10 10.5v5M14 10.5v5" />
      </>
    ),
  };

  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      {paths[action]}
    </svg>
  );
}
