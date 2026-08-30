import { serviceJobLabel, type ServiceJob } from "../domain/serviceMonitor";

interface ServiceJobListProps {
  jobs: ServiceJob[];
  selectedJobId: string | null;
  onSelect(id: string): void;
}

export function ServiceJobList({
  jobs,
  selectedJobId,
  onSelect,
}: ServiceJobListProps) {
  if (!jobs.length) {
    return (
      <p className="v2-service-empty">Пока нет запросов на распознавание.</p>
    );
  }

  return (
    <ul className="v2-service-job-list">
      {jobs.map((job) => (
        <li key={job.id}>
          <button
            type="button"
            className={job.id === selectedJobId ? "is-selected" : ""}
            onClick={() => onSelect(job.id)}
          >
            <span className={`v2-service-job-state is-${job.state}`} />
            <span>
              <strong>{shortId(job.id)}</strong>
              <small>
                {serviceJobLabel(job.state)} · {formatTime(job.createdAtMs)}
              </small>
              {job.result && (
                <small className="v2-service-job-meta">
                  {job.result.model} · {job.result.backend ?? "CPU"}
                </small>
              )}
            </span>
            <em>{formatJobTiming(job)}</em>
          </button>
        </li>
      ))}
    </ul>
  );
}

export function JobSummary({ job }: { job: ServiceJob }) {
  return (
    <div className="v2-service-job-summary">
      <span className={`v2-service-job-state is-${job.state}`} />
      <div>
        <strong>{shortId(job.id)}</strong>
        <p>
          {serviceJobLabel(job.state)} · отправлено{" "}
          {formatTime(job.createdAtMs)}
        </p>
      </div>
      <span>
        {job.state === "queued"
          ? "Ожидает свободный pipeline"
          : "Распознаётся в общем pipeline"}
      </span>
    </div>
  );
}

export function JobDetails({ job }: { job: ServiceJob | null }) {
  if (!job) {
    return (
      <p className="v2-service-empty">
        Выберите строку слева, чтобы посмотреть детали.
      </p>
    );
  }
  if (job.error) return <p className="v2-service-error">{job.error}</p>;
  if (!job.result) {
    return (
      <p className="v2-service-empty">
        {serviceJobLabel(job.state)}. Текст появится после завершения.
      </p>
    );
  }

  return (
    <div className="v2-service-result">
      <dl>
        <div>
          <dt>Аудио</dt>
          <dd>{formatSeconds(job.result.audioSeconds)}</dd>
        </div>
        <div>
          <dt>Распознавание</dt>
          <dd>{formatSeconds(job.result.transcribeSeconds)}</dd>
        </div>
        <div>
          <dt>Backend</dt>
          <dd>{job.result.backend ?? "—"}</dd>
        </div>
        <div>
          <dt>Модель</dt>
          <dd>{job.result.model}</dd>
        </div>
        <div>
          <dt>Язык запроса</dt>
          <dd>{job.requestedLanguage}</dd>
        </div>
        {job.result.detectedLanguage && (
          <div>
            <dt>Определён</dt>
            <dd>{job.result.detectedLanguage}</dd>
          </div>
        )}
      </dl>
      <p>{job.result.text}</p>
    </div>
  );
}

function shortId(id: string) {
  const suffix = id.replace(/^tr_/, "");
  return suffix.length > 6 ? `#…${suffix.slice(-6)}` : `#${suffix}`;
}

function formatTime(timestamp: number) {
  return new Intl.DateTimeFormat("ru-RU", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(timestamp);
}

function formatSeconds(value: number | null) {
  if (value === null) return "—";
  return value < 1
    ? `${Math.round(value * 1_000)} мс`
    : `${value.toFixed(1)} с`;
}

function formatJobTiming(job: ServiceJob) {
  return job.result
    ? formatSeconds(job.result.transcribeSeconds)
    : serviceJobLabel(job.state);
}
