import { useMemo, useState } from "react";
import type { ServiceRuntime } from "../application/serviceRuntime";
import { useServiceMonitor } from "../application/useServiceMonitor";
import { isActiveServiceJob } from "../domain/serviceMonitor";
import { JobDetails, JobSummary, ServiceJobList } from "./ServiceJobViews";
import { PageFrame } from "@/v2/shared/presentation/components/PageFrame";
import { StatusChip } from "@/v2/shared/presentation/components/StatusChip";

interface ServicePageProps {
  runtime: ServiceRuntime;
}

export function ServicePage({ runtime }: ServicePageProps) {
  const { snapshot, error, isRefreshing, refresh, cancelJob, copyText } =
    useServiceMonitor(runtime);
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">(
    "idle",
  );
  const selectedJob = useMemo(
    () =>
      snapshot?.queue.jobs.find((job) => job.id === selectedJobId) ??
      snapshot?.queue.jobs[0] ??
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

            <section
              className="v2-service-metrics"
              aria-label="Метрики очереди"
            >
              <Metric
                label="В очереди"
                value={`${snapshot.queue.queued} / ${snapshot.queue.capacity}`}
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
              <Metric label="Готово" value={String(snapshot.queue.completed)} />
              <Metric
                label="Ошибки"
                value={String(snapshot.queue.failed)}
                tone={snapshot.queue.failed ? "danger" : "default"}
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
                    <p className="v2-kicker">Текущий запуск</p>
                    <h2>Последние задачи</h2>
                  </div>
                  <span>В памяти: {snapshot.queue.jobs.length}</span>
                </header>
                <ServiceJobList
                  jobs={snapshot.queue.jobs}
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
                        ? selectedJob.id.replace("tr_", "#").slice(0, 10)
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
              Последние 50 завершённых задач хранятся только в памяти до
              закрытия Fono.
            </p>
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
