import type { DictationHistoryEntry, SpeechFinding } from "@/lib/types";

export function SpeechSessionDetails({
  entry,
}: {
  entry: DictationHistoryEntry | null;
}) {
  if (!entry?.analysis) {
    return (
      <section className="v2-speech-trainer-panel v2-speech-trainer-panel--details">
        <p className="v2-speech-trainer-panel__empty">
          Выберите готовую сессию.
        </p>
      </section>
    );
  }

  return (
    <section className="v2-speech-trainer-panel v2-speech-trainer-panel--details">
      <header>
        <div>
          <p className="v2-kicker">Сессия</p>
          <h2>Что заметил Fono</h2>
        </div>
        <span>{entry.device ?? "Локально"}</span>
      </header>
      <dl className="v2-speech-trainer-session-metrics">
        <div>
          <dt>Паразиты</dt>
          <dd>{entry.analysis.filler_count}</dd>
        </div>
        <div>
          <dt>Повторы</dt>
          <dd>{entry.analysis.repetition_count}</dd>
        </div>
        <div>
          <dt>Самопоправки</dt>
          <dd>{entry.analysis.self_correction_count}</dd>
        </div>
        <div>
          <dt>Обрывки</dt>
          <dd>{entry.analysis.unfinished_count}</dd>
        </div>
      </dl>
      <div className="v2-speech-trainer-transcripts">
        <Transcript
          label="Исходная расшифровка"
          text={entry.original_text ?? "Недоступна: срок хранения истёк."}
        />
        <Transcript label="Вставленный текст" text={entry.text} />
      </div>
      {entry.analysis.findings.length ? (
        <ul className="v2-speech-trainer-findings">
          {entry.analysis.findings.map((finding, index) => (
            <Finding
              key={`${finding.kind}-${finding.start_word}-${index}`}
              finding={finding}
            />
          ))}
        </ul>
      ) : (
        <p className="v2-speech-trainer-panel__empty">
          В этой сессии заметных паттернов нет.
        </p>
      )}
    </section>
  );
}

export function describeSpeechSession(entry: DictationHistoryEntry) {
  const analysis = entry.analysis;
  if (!analysis) return "Анализ недоступен";
  const total = analysis.findings.length;
  return total
    ? `${total} наблюд. · ${analysis.filler_density_per_100_words.toFixed(1)} / 100`
    : "Без заметных паттернов";
}

function Transcript({ label, text }: { label: string; text: string }) {
  return (
    <div>
      <span>{label}</span>
      <p>{text}</p>
    </div>
  );
}

function Finding({ finding }: { finding: SpeechFinding }) {
  return (
    <li>
      <strong>{finding.label}</strong>
      <span>«{finding.fragment}»</span>
    </li>
  );
}
