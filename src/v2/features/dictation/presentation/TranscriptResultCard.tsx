interface TranscriptResultCardProps {
  transcript: string;
  isProcessing: boolean;
}

export function TranscriptResultCard({
  transcript,
  isProcessing,
}: TranscriptResultCardProps) {
  const hasTranscript = transcript.trim().length > 0;

  return (
    <section
      className={`v2-transcript-result-card ${
        hasTranscript ? "has-result" : ""
      }`}
      aria-live="polite"
    >
      <header>
        <span>Распознанный текст</span>
        <small>
          {isProcessing
            ? "Обрабатываю…"
            : hasTranscript
              ? "Готово"
              : "Ожидание"}
        </small>
      </header>
      <p>
        {hasTranscript
          ? transcript
          : "Здесь появится текст после завершения диктовки."}
      </p>
    </section>
  );
}
