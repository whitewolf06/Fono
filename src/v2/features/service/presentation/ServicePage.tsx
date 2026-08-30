import { useMemo, useState } from "react";
import type { ServiceRuntime } from "../application/serviceRuntime";
import { useServiceMonitor } from "../application/useServiceMonitor";
import { isActiveServiceJob } from "../domain/serviceMonitor";
import { ServiceApiPanel } from "./ServiceApiPanel";
import { JobDetails, JobSummary, ServiceJobList } from "./ServiceJobViews";
import { PageFrame } from "@/v2/shared/presentation/components/PageFrame";
import { StatusChip } from "@/v2/shared/presentation/components/StatusChip";

interface ServicePageProps {
  runtime: ServiceRuntime;
}

export function ServicePage({ runtime }: ServicePageProps) {
  const {
    snapshot,
    error,
    isRefreshing,
    refresh,
    cancelJob,
    clearHistory,
    copyText,
    copyApiToken,
  } = useServiceMonitor(runtime);
  const [tab, setTab] = useState<"overview" | "api">("overview");
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">(
    "idle",
  );
  const selectedJob = useMemo(
    () =>
      snapshot?.history.jobs.find((job) => job.id === selectedJobId) ??
      snapshot?.history.jobs[0] ??
      null,
    [selectedJobId, snapshot],
  );
  const activeJob = snapshot?.queue.jobs.find(isActiveServiceJob) ?? null;

  const handleCopy = async () => {
    if (!selectedJob?.result?.text) return;
    try {
      await copyText(selectedJob.result.text);
      setCopyState("copied");
    } catch {
      setCopyState("error");
    }
  };

  return (
    <PageFrame
      icon={
        <span className="v2-service-page__icon" aria-hidden="true">
          ⌁
        </span>
      }
      title="Сервис"
      description="Локальный REST API, его очередь и результаты текущего запуска."
    >
      <div className="v2-service-page">
        {error ? (
          <section className="v2-service-alert" role="status">
            <strong>Не удалось получить состояние сервиса</strong>
            <span>{error}</span>
            <button
              className="v2-button"
              type="button"
              onClick={() => void refresh()}
            >
              Повторить
            </button>
          </section>
        ) : !snapshot ? (
          <div className="v2-loading">
            Получаю состояние локального сервиса…
          </div>
        ) : (
          <>
            <section
              className="v2-service-connection"
              aria-label="Состояние REST API"
            >
              <div>
                <StatusChip tone="ready">
                  Локальный REST API работает
                </StatusChip>
                <strong>{snapshot.address}</strong>
                <span>Protocol v{snapshot.protocolVersion}</span>
              </div>
              <dl>
                <div>
                  <dt>Модель</dt>
                  <dd>{snapshot.model}</dd>
                </div>
                <div>
                  <dt>Ускорение</dt>
                  <dd>{snapshot.acceleration}</dd>
                </div>
              </dl>
              <button
                className="v2-button"
                type="button"
                disabled={isRefreshing}
                onClick={() => void refresh()}
              >
                {isRefreshing ? "Обновляю…" : "Обновить"}
              </button>
            </section>

            <nav className="v2-service-tabs" aria-label="Раздел сервиса">
              <button
                className={tab === "overview" ? "is-active" : ""}
                type="button"
                onClick={() => setTab("overview")}
              >
                Обзор
              </button>
              <button
                className={tab === "api" ? "is-active" : ""}
                type="button"
                onClick={() => setTab("api")}
              >
                API
              </button>
            </nav>

            {tab === "api" ? (
              <ServiceApiPanel
                address={snapshot.address}
                onCopy={(text) => void copyText(text)}
                onCopyApiToken={copyApiToken}
              />
            ) : (
              <>
                <section
                  className="v2-service-metrics"
                  aria-label="Метрики очереди"
                >
                  <Metric
                    label="В очереди"
                    value={String(snapshot.queue.queued)}
                    detail={`Лимит очереди: ${snapshot.queue.capacity}`}
                  />
                  <Metric
                    label="В работе"
                    value={String(
                      snapshot.queue.preparing + snapshot.queue.transcribing,
                    )}
                    detail={
                      activeJob ? "Есть активная задача" : "Нет активной задачи"
                    }
                  />
                  <Metric
                    label="Готово"
                    value={
                      snapshot.history.completed
                        ? String(snapshot.history.completed)
                        : "—"
                    }
                    detail={
                      snapshot.history.completed
                        ? "В локальной истории"
                        : "Пока нет результатов"
                    }
                  />
                  <Metric
                    label="Скорость"
                    value={formatSpeedValue(
                      snapshot.history.totalAudioSeconds,
                      snapshot.history.totalTranscribeSeconds,
                    )}
                    detail={formatSpeed(
                      snapshot.history.totalAudioSeconds,
                      snapshot.history.totalTranscribeSeconds,
                    )}
                  />
                  <Metric
                    label="Ошибки"
                    value={
                      snapshot.history.failed
                        ? String(snapshot.history.failed)
                        : "—"
                    }
                    detail={
                      snapshot.history.failed
                        ? "Требуют проверки"
                        : "Ошибок нет"
                    }
                    tone={snapshot.history.failed ? "danger" : "default"}
                  />
                </section>

                <section className="v2-service-panel">
                  <header>
                    <div>
                      <p className="v2-kicker">Сейчас</p>
                      <h2>Текущая работа</h2>
                    </div>
                    {activeJob && (
                      <button
                        className="v2-button v2-button--quiet"
                        type="button"
                        onClick={() => void cancelJob(activeJob.id)}
                      >
                        Отменить задачу
                      </button>
                    )}
                  </header>
                  {activeJob ? (
                    <JobSummary job={activeJob} />
                  ) : (
                    <p className="v2-service-empty">
                      Очередь свободна. Новые REST-запросы появятся здесь.
                    </p>
                  )}
                </section>

                <div className="v2-service-content-grid">
                  <section className="v2-service-panel v2-service-panel--jobs">
                    <header>
                      <div>
                        <p className="v2-kicker">Локальная история</p>
                        <h2>Последние результаты</h2>
                      </div>
                      {snapshot.history.jobs.length ? (
                        <button
                          className="v2-button v2-button--quiet"
                          type="button"
                          onClick={() => void clearHistory()}
                        >
                          Очистить
                        </button>
                      ) : (
                        <span>Пока пусто</span>
                      )}
                    </header>
                    <ServiceJobList
                      jobs={snapshot.history.jobs}
                      selectedJobId={selectedJob?.id ?? null}
                      onSelect={(id) => {
                        setSelectedJobId(id);
                        setCopyState("idle");
                      }}
                    />
                  </section>

                  <section className="v2-service-panel v2-service-panel--result">
                    <header>
                      <div>
                        <p className="v2-kicker">Результат</p>
                        <h2>
                          {selectedJob
                            ? formatJobId(selectedJob.id)
                            : "Выберите задачу"}
                        </h2>
                      </div>
                      {selectedJob?.result?.text && (
                        <button
                          className="v2-button"
                          type="button"
                          onClick={() => void handleCopy()}
                        >
                          {copyState === "copied"
                            ? "Скопировано"
                            : copyState === "error"
                              ? "Повторить"
                              : "Копировать"}
                        </button>
                      )}
                    </header>
                    <JobDetails job={selectedJob} />
                  </section>
                </div>

                <p className="v2-service-privacy-note">
                  До 100 завершённых REST-задач хранятся локально в Fono,
                  включая текст результата.
                </p>
              </>
            )}
          </>
        )}
      </div>
    </PageFrame>
  );
}

function formatSpeed(audioSeconds: number, transcribeSeconds: number) {
  if (!audioSeconds || !transcribeSeconds)
    return "Скорость появится после результата";
  return `${(audioSeconds / transcribeSeconds).toFixed(1)}× реального времени`;
}

function formatSpeedValue(audioSeconds: number, transcribeSeconds: number) {
  if (!audioSeconds || !transcribeSeconds) return "—";
  return `${(audioSeconds / transcribeSeconds).toFixed(1)}×`;
}

function formatJobId(id: string) {
  const suffix = id.replace(/^tr_/, "");
  return suffix.length > 6 ? `#…${suffix.slice(-6)}` : `#${suffix}`;
}

function Metric({
  label,
  value,
  detail,
  tone = "default",
}: {
  label: string;
  value: string;
  detail?: string;
  tone?: "default" | "danger";
}) {
  return (
    <article className={`v2-service-metric is-${tone}`}>
      <span>{label}</span>
      <strong>{value}</strong>
      {detail && <small>{detail}</small>}
    </article>
  );
}
