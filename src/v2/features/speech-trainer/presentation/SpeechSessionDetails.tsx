import type { DictationHistoryEntry, SpeechFinding } from "@/lib/types";

export function SpeechSessionDetails({
  entry,
  isUpdating,
  onSetAnalyticsIncluded,
}: {
  entry: DictationHistoryEntry | null;
  isUpdating: boolean;
  onSetAnalyticsIncluded: (included: boolean) => void;
}) {
  if (!entry) {
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
      <div className="v2-speech-trainer-inclusion">
        <div>
          <strong>
            {entry.analytics_included
              ? "Учитывается в отчёте"
              : "Исключено из отчёта"}
          </strong>
          <span>
            {entry.analytics_included
              ? "Метрики этой записи входят в статистику периода."
              : "Запись сохранена, но не влияет на статистику; незапущенный анализ будет пропущен."}
          </span>
        </div>
        <button
          className="v2-button v2-button--quiet"
          type="button"
          disabled={isUpdating}
          onClick={() => onSetAnalyticsIncluded(!entry.analytics_included)}
        >
          {isUpdating
            ? "Сохраняю…"
            : entry.analytics_included
              ? "Не учитывать"
              : "Вернуть в отчёт"}
        </button>
      </div>
      {entry.analysis ? (
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
      ) : (
        <p className="v2-speech-trainer-panel__empty">
          Анализ этой записи ещё готовится.
        </p>
      )}
      <div className="v2-speech-trainer-transcripts">
        <Transcript
          label="Исходная расшифровка"
          text={entry.original_text ?? "Недоступна: срок хранения истёк."}
        />
        <Transcript label="Вставленный текст" text={entry.text} />
      </div>
      {entry.analysis?.findings.length ? (
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
      <Recommendation entry={entry} />
    </section>
  );
}

function Recommendation({ entry }: { entry: DictationHistoryEntry }) {
  if (entry.recommendation_status === "pending") {
    return (
      <p className="v2-speech-trainer-panel__empty">Готовим рекомендации…</p>
    );
  }
  if (entry.recommendation_status === "failed") {
    return (
      <p className="v2-speech-trainer-panel__empty">
        Рекомендации сейчас недоступны.
      </p>
    );
  }
  if (!entry.recommendation) return null;

  return (
    <section className="v2-speech-trainer-recommendation">
      <p className="v2-kicker">LLM-рекомендации</p>
      <p>{entry.recommendation.summary}</p>
      <ul>
        {entry.recommendation.recommendations.map((item, index) => (
          <li key={`${item.title}-${index}`}>
            <strong>{item.title}</strong>
            <span>{item.observation}</span>
            <em>{item.exercise}</em>
          </li>
        ))}
      </ul>
    </section>
  );
}

export function describeSpeechSession(entry: DictationHistoryEntry) {
  if (!entry.analytics_included) return "Не учитывается в отчёте";
  const analysis = entry.analysis;
  if (!analysis)
    return entry.analysis_status === "pending"
      ? "Анализируется"
      : "Анализ недоступен";
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
