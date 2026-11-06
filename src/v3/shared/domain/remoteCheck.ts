import { message } from "./messages";
import type { Phase } from "./contracts";

export type RemoteCheckReason =
  | "network"
  | "timeout"
  | "unauthorized"
  | "not_configured"
  | "incompatible_version"
  | "unsupported_capabilities"
  | "invalid_response";
export type RemoteCheckSnapshot = { operation_id: number } & (
  | {
      state:
        | "checking"
        | "ready"
        | "starting"
        | "stopping"
        | "busy"
        | "model_not_ready"
        | "cancelled";
    }
  | { state: "unavailable"; reason: RemoteCheckReason }
);

const states = new Set([
  "checking",
  "ready",
  "starting",
  "stopping",
  "busy",
  "model_not_ready",
  "cancelled",
]);
const reasons = new Set([
  "network",
  "timeout",
  "unauthorized",
  "not_configured",
  "incompatible_version",
  "unsupported_capabilities",
  "invalid_response",
]);

/** Only bounded, operation-scoped protocol values reach the status line. */
export function isRemoteCheckSnapshot(
  value: unknown,
): value is RemoteCheckSnapshot {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Record<string, unknown>;
  return (
    Number.isSafeInteger(candidate.operation_id) &&
    Number(candidate.operation_id) > 0 &&
    typeof candidate.state === "string" &&
    (states.has(candidate.state) ||
      (candidate.state === "unavailable" &&
        typeof candidate.reason === "string" &&
        reasons.has(candidate.reason)))
  );
}

export function remoteCheckStatus(
  check: RemoteCheckSnapshot | null | undefined,
  operation: number | null | undefined,
  phase: Phase,
  fallback?: "none" | "ask" | "local" | null,
): string | undefined {
  if (
    !isRemoteCheckSnapshot(check) ||
    check.operation_id !== operation ||
    !["listening", "silence"].includes(phase)
  )
    return undefined;
  if (check.state === "unavailable") {
    if (check.reason === "incompatible_version")
      return message("recognitionSource.incompatible");
    if (check.reason === "unsupported_capabilities")
      return message("recognitionSource.unsupportedCapabilities");
  }
  if (
    ["starting", "stopping", "model_not_ready", "unavailable"].includes(
      check.state,
    )
  ) {
    if (fallback === "local") return message("remoteCheck.localFallback");
    if (fallback === "ask") return message("remoteCheck.askFallback");
  }
  switch (check.state) {
    case "checking":
      return message("remoteCheck.checking");
    case "ready":
      return message("remoteCheck.ready");
    case "starting":
      return message("remoteCheck.starting");
    case "stopping":
      return message("remoteCheck.stopping");
    case "busy":
      return message("remoteCheck.busy");
    case "model_not_ready":
      return message("remoteCheck.modelNotReady");
    case "cancelled":
      return undefined;
    case "unavailable":
      switch (check.reason) {
        case "timeout":
          return message("remoteCheck.timeout");
        case "unauthorized":
          return message("remoteCheck.unauthorized");
        case "not_configured":
          return message("remoteCheck.notConfigured");
        case "invalid_response":
          return message("remoteCheck.invalidResponse");
        default:
          return message("remoteCheck.unavailable");
      }
  }
}
