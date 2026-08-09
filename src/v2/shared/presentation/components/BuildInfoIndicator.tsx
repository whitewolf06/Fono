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
  const isSynchronized =
    backend !== null &&
    frontend.revision !== "unknown" &&
    backend.revision !== "unknown" &&
    frontend.revision === backend.revision;
  const status = !isNativeRuntime
    ? "UI mock"
    : backendError
      ? "Backend unavailable"
      : isSynchronized
        ? "Versions match"
        : "Versions differ";

  return (
    <section className="v2-build-info" aria-label="Build versions">
      <div className="v2-build-info__status">
        <span
          className={
            !isNativeRuntime || backendError || !isSynchronized
              ? "is-warning"
              : "is-ready"
          }
        />
        {status}
      </div>
      <div className="v2-build-info__row" title={`Frontend ${frontend.revision}`}>
        <span>FE</span>
        <code>v{frontend.version}+{frontend.revision}</code>
      </div>
      <div
        className="v2-build-info__row"
        title={backend ? `Backend ${backend.revision}` : undefined}
      >
        <span>BE</span>
        <code>
          {backend
            ? `v${backend.version}+${backend.revision} (${backend.profile})`
            : isNativeRuntime
              ? "loading…"
              : "not running"}
        </code>
      </div>
    </section>
  );
}
