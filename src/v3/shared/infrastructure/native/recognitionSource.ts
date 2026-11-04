import type {
  RecognitionSourcePort,
  RemoteConnectionError,
  RemoteConnectionResult,
  RemoteServerInfo,
} from "../../domain/recognitionSource";
import { validRemoteEndpoint } from "../../domain/recognitionSource";
import { modelId, modelKey } from "../../domain/speechModels";
import type { ModelId } from "./ipcTypes";
import { call } from "./ipc";
import { message } from "../../domain/messages";
import { NativeIpcError } from "./nativeErrors";

const errors = new Set<RemoteConnectionError>([
  "invalid_endpoint",
  "not_configured",
  "network",
  "timeout",
  "unauthorized",
  "incompatible_version",
  "model_not_found",
  "invalid_response",
  "busy",
  "server_failed",
]);
const bounded = (value: unknown, max: number): value is string =>
  typeof value === "string" &&
  value.length <= max &&
  ![...value].some(
    (character) =>
      character.charCodeAt(0) < 32 || character.charCodeAt(0) === 127,
  );
function validModelId(value: unknown): value is ModelId {
  if (!value || typeof value !== "object") return false;
  const item = value as ModelId;
  return (
    ["whisper", "giga_am", "parakeet"].includes(item.engine) &&
    typeof item.variant === "string" &&
    /^[A-Za-z0-9_.-]{1,128}$/.test(item.variant)
  );
}
/** Whitelist the IPC reply; never spread arbitrary server fields into UI state. */
export function remoteConnectionFromNative(
  value: unknown,
): RemoteConnectionResult {
  const invalid = (): RemoteConnectionResult => ({
    kind: "error",
    error: "invalid_response",
  });
  if (!value || typeof value !== "object") return invalid();
  const reply = value as Record<string, unknown>;
  if (reply.kind === "error")
    return typeof reply.error === "string" &&
      errors.has(reply.error as RemoteConnectionError)
      ? { kind: "error", error: reply.error as RemoteConnectionError }
      : invalid();
  if (reply.kind !== "ok" || !reply.server || typeof reply.server !== "object")
    return invalid();
  const s = reply.server as Record<string, unknown>;
  if (
    !bounded(s.version, 64) ||
    !Number.isInteger(s.api_version) ||
    Number(s.api_version) < 2 ||
    !["ready", "starting", "busy", "failed", "stopping"].includes(
      String(s.state),
    ) ||
    !Array.isArray(s.models) ||
    s.models.length > 256 ||
    !Array.isArray(s.devices) ||
    s.devices.length > 64
  )
    return invalid();
  const server: RemoteServerInfo = {
    version: s.version,
    apiVersion: Number(s.api_version),
    state: s.state as RemoteServerInfo["state"],
    devices: [],
    models: [],
  };
  for (const raw of s.devices) {
    if (
      !raw ||
      typeof raw !== "object" ||
      !bounded(raw.id, 64) ||
      !bounded(raw.name, 160) ||
      !bounded(raw.backend, 32) ||
      typeof raw.available !== "boolean"
    )
      return invalid();
    server.devices.push({
      id: raw.id,
      name: raw.name,
      backend: raw.backend,
      available: raw.available,
    });
  }
  for (const raw of s.models) {
    if (
      !raw ||
      typeof raw !== "object" ||
      !validModelId(raw.id) ||
      !bounded(raw.label, 160) ||
      typeof raw.installed !== "boolean" ||
      !bounded(raw.state, 32) ||
      !bounded(raw.device, 64) ||
      !Array.isArray(raw.languages) ||
      raw.languages.length > 64 ||
      !raw.languages.every((l: unknown) => bounded(l, 16))
    )
      return invalid();
    server.models.push({
      id: modelKey(raw.id),
      label: raw.label,
      installed: raw.installed,
      state: raw.state,
      device: raw.device,
      languages: [...raw.languages],
    });
  }
  return { kind: "ok", server };
}
export function createNativeRecognitionSource(
  invoke: typeof call = call,
  serialize: <T>(action: () => Promise<T>) => Promise<T> = (action) => action(),
): RecognitionSourcePort {
  return {
    async status() {
      const value = await invoke<{ has_token: boolean }>(
        "get_remote_stt_status",
      );
      return { hasToken: value?.has_token === true };
    },
    async test(input) {
      if (!validRemoteEndpoint(input.url))
        return { kind: "error", error: "invalid_endpoint" };
      try {
        return remoteConnectionFromNative(
          await invoke("test_remote_stt_server", {
            request: {
              url: input.url.trim(),
              ...(input.token ? { token: input.token } : {}),
              ...(input.caPem ? { ca_pem: input.caPem } : {}),
              ...(input.model ? { model: modelId(input.model) } : {}),
            },
          }),
        );
      } catch (error) {
        if (error instanceof NativeIpcError && error.code === "busy")
          return { kind: "error", error: "busy" };
        throw error;
      }
    },
    async saveToken(token) {
      if (!/^[A-Za-z0-9._~+/=-]{1,4096}$/.test(token))
        throw new Error(message("recognitionSource.invalidToken"));
      await serialize(() => invoke<void>("set_remote_stt_token", { token }));
    },
    async clearToken() {
      await serialize(() => invoke<void>("clear_remote_stt_token"));
    },
  };
}
