import { useEffect, useMemo, useState } from "react";
import type { SpeechTrainerStore } from "../application/useSpeechTrainer";
import { useSpeechTrainer } from "../application/useSpeechTrainer";
import {
  describeSpeechSession,
  SpeechSessionDetails,
} from "./SpeechSessionDetails";
import {
  formatDensity,
  hasSpeechFindings,
  speechTrainerPeriods,
} from "../domain/speechTrainer";
import { PageFrame } from "@/v2/shared/presentation/components/PageFrame";

interface SpeechTrainerPageProps {
  store: SpeechTrainerStore;
  onOpenPrivacySettings: () => void;
}

export function SpeechTrainerPage({
  store,
  onOpenPrivacySettings,
}: SpeechTrainerPageProps) {
  const trainer = useSpeechTrainer(store);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [clearError, setClearError] = useState<string | null>(null);
  const analyzedEntries = useMemo(
    () =>
      trainer.entries
        .filter((entry) => entry.analysis_status === "ready" && entry.analysis)
        .sort(
          (left, right) =>
            new Date(right.created_at).getTime() -
            new Date(left.created_at).getTime(),
        ),
    [trainer.entries],
  );
  const selectedEntry =
    analyzedEntries.find((entry) => entry.id === selectedId) ??
    analyzedEntries[0] ??
    null;

  useEffect(() => {
    if (
      selectedId &&
      !analyzedEntries.some((entry) => entry.id === selectedId)
    ) {
      setSelectedId(null);
    }
  }, [analyzedEntries, selectedId]);

  const clearHistory = async () => {
    setClearError(null);
    try {
      await trainer.clearHistory();
    } catch (reason) {
      setClearError(
        reason instanceof Error
          ? reason.message
          : "Не удалось очистить данные.",
      );
    }
  };

  return (
    <PageFrame
      icon={<span className="v2-speech-trainer-page__icon">◌</span>}
      title="Речевой тренер"
      description="Локальный разбор сохранённых диктовок — без отправки расшифровок во внешние сервисы."
    >
      <div className="v2-speech-trainer-page">
        <section className="v2-speech-trainer-toolbar">
          <div>
            <p className="v2-kicker">Период отчёта</p>
            <label>
              <span className="v2-visually-hidden">Выберите период</span>
              <select
                value={trainer.period}
                onChange={(event) =>
                  trainer.setPeriod(Number(event.target.value) as 7 | 30 | 90)
                }
              >
                {speechTrainerPeriods.map((period) => (
                  <option key={period.value} value={period.value}>
                    {period.label}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <div className="v2-speech-trainer-toolbar__actions">
            <button
              className="v2-button v2-button--quiet"
              type="button"
              disabled={trainer.isRefreshing}
              onClick={() => void trainer.refresh()}
            >
              {trainer.isRefreshing ? "Обновляю…" : "Обновить"}
            </button>
            <button
              className="v2-button"
              type="button"
              onClick={onOpenPrivacySettings}
            >
              Данные и приватность
            </button>
          </div>
        </section>

        {trainer.isLoading ? (
          <div className="v2-loading">Собираю локальный отчёт…</div>
        ) : trainer.error ? (
          <section className="v2-speech-trainer-alert" role="status">
            <strong>Не удалось получить отчёт</strong>
            <span>{trainer.error}</span>
            <button
              className="v2-button"
              type="button"
              onClick={() => void trainer.refresh()}
            >
              Повторить
            </button>
          </section>
        ) : !trainer.analyticsEnabled ? (
          <section className="v2-speech-trainer-empty">
            <strong>Аналитика речи выключена</strong>
            <p>
              Чтобы построить отчёт, Fono должен локально хранить исходную
              расшифровку выбранный срок. Внешние сервисы для этого не
              используются.
            </p>
            <button
              className="v2-button v2-button--primary"
              type="button"
              onClick={onOpenPrivacySettings}
            >
              Открыть настройки приватности
            </button>
          </section>
        ) : !trainer.report || !trainer.report.analyzed_sessions ? (
          <section className="v2-speech-trainer-empty">
            <strong>Пока нет готовых сессий для отчёта</strong>
            <p>
              Запишите диктовку. После вставки Fono обработает её в фоне и
              добавит локальные метрики в этот раздел.
            </p>
          </section>
        ) : (
          <>
            <section
              className="v2-speech-trainer-metrics"
              aria-label="Метрики речи"
            >
              <Metric
                label="Сессии"
                value={String(trainer.report.analyzed_sessions)}
                detail="Готовы к разбору"
              />
              <Metric
                label="Слова"
                value={String(trainer.report.total_words)}
                detail="В исходной речи"
              />
              <Metric
                label="Паразиты"
                value={formatDensity(
                  trainer.report.filler_density_per_100_words,
                )}
                detail="Слов на 100 слов"
              />
              <Metric
                label="Повторы"
                value={String(trainer.report.repetition_count)}
                detail="Соседние слова"
              />
              <Metric
                label="Самопоправки"
                value={String(trainer.report.self_correction_count)}
                detail="Нейтральное наблюдение"
              />
            </section>

            <div className="v2-speech-trainer-grid">
              <section className="v2-speech-trainer-panel">
                <header>
                  <div>
                    <p className="v2-kicker">Сессии</p>
                    <h2>Последние разборы</h2>
                  </div>
                  <span>{analyzedEntries.length}</span>
                </header>
                <div className="v2-speech-trainer-session-list">
                  {analyzedEntries.map((entry) => (
                    <button
                      key={entry.id}
                      className={
                        entry.id === selectedEntry?.id ? "is-selected" : ""
                      }
                      type="button"
                      onClick={() => setSelectedId(entry.id)}
                    >
                      <span>{new Date(entry.created_at).toLocaleString()}</span>
                      <strong>{entry.analysis?.word_count ?? 0} слов</strong>
                      <small>{describeSpeechSession(entry)}</small>
                    </button>
                  ))}
                </div>
              </section>

              <SpeechSessionDetails entry={selectedEntry} />
            </div>

            <section className="v2-speech-trainer-insight">
              <div>
                <p className="v2-kicker">Практика</p>
                <h2>{exerciseTitle(trainer.report)}</h2>
                <p>{exerciseDescription(trainer.report)}</p>
              </div>
              <span>
                Это наблюдения о фрагментах речи, а не оценка вас или ваших
                способностей.
              </span>
            </section>

            <footer className="v2-speech-trainer-footer">
              <span>
                Исходные расшифровки и результаты остаются на этом устройстве.
              </span>
              <button
                className="v2-button v2-button--quiet"
                type="button"
                onClick={() => void clearHistory()}
              >
                Удалить историю и аналитику
              </button>
              {clearError && <strong role="status">{clearError}</strong>}
            </footer>
          </>
        )}
      </div>
    </PageFrame>
  );
}

function Metric({
  label,
  value,
  detail,
}: {
  label: string;
  value: string;
  detail: string;
}) {
  return (
    <article>
      <span>{label}</span>
      <strong>{value}</strong>
      <small>{detail}</small>
    </article>
  );
}

function exerciseTitle(
  report: NonNullable<ReturnType<typeof useSpeechTrainer>["report"]>,
) {
  return report.filler_count
    ? "Попробуйте делать короткую паузу вместо слова-паразита"
    : "Речь в этом периоде звучит ровно";
}

function exerciseDescription(
  report: NonNullable<ReturnType<typeof useSpeechTrainer>["report"]>,
) {
  if (!hasSpeechFindings(report))
    return "Продолжайте в том же темпе. Новые сессии помогут увидеть устойчивую динамику.";
  if (report.repetition_count + report.self_correction_count)
    return "Перед следующей мыслью сделайте вдох и сформулируйте её целиком. Это помогает снизить повторы и самопоправки без искусственного контроля речи.";
  return "Выберите одну короткую запись и попробуйте повторить её, заменяя слова-паразиты спокойной паузой.";
}
