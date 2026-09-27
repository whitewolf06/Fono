import type { BuildInfo } from "@/lib/types";

interface BuildInfoIndicatorProps {
  backend: BuildInfo | null;
  backendError: boolean;
  isNativeRuntime: boolean;
}

const frontend = __FONO_FRONTEND_BUILD__;

export function BuildInfoIndicator({
  backend,
  backendError,
  isNativeRuntime,
}: BuildInfoIndicatorProps) {
  const versionsDiffer =
    backend !== null && backend.version !== frontend.version;
  const revisionsDiffer =
    backend !== null &&
    frontend.revision !== "unknown" &&
    backend.revision !== "unknown" &&
    backend.revision !== frontend.revision;
  const warning = backendError
    ? "Не удалось проверить версию ядра"
    : versionsDiffer
      ? "Версии интерфейса и ядра различаются"
      : revisionsDiffer
        ? "Сборки интерфейса и ядра различаются"
        : null;

  return (
    <section className="v2-build-info" aria-label="Версия приложения">
      <div className="v2-build-info__version">
        <span>Fono</span>
        <strong>v{frontend.version}</strong>
      </div>
      {warning && (
        <p className="v2-build-info__warning" role="status">
          {warning}
        </p>
      )}
      <details className="v2-build-info__details">
        <summary>Сведения о сборке</summary>
        <div
          className="v2-build-info__row"
          title={`Frontend ${frontend.revision}`}
        >
          <span>Интерфейс</span>
          <code>
            v{frontend.version} · {frontend.revision}
          </code>
        </div>
        <div className="v2-build-info__row" title={backend?.revision}>
          <span>Ядро</span>
          <code>
            {backend
              ? `v${backend.version} · ${backend.revision} (${backend.profile})`
              : isNativeRuntime
                ? backendError
                  ? "недоступно"
                  : "загрузка…"
                : "UI mock"}
          </code>
        </div>
      </details>
    </section>
  );
}
