import catalog from "./userErrorCatalog.json";
import { createMessageDictionary } from "../../domain/messages";
const errorMessage = createMessageDictionary(catalog.messages);

export type NativeErrorCode = keyof typeof catalog.messages;
type RecognitionBackend = "auto" | "cpu" | "cuda" | "vulkan";
interface NativeErrorParams {
  detail?: string;
  backend?: RecognitionBackend;
}

const backendLabels: Record<RecognitionBackend, string> = {
  auto: "Авто",
  cpu: "Процессор",
  cuda: "CUDA",
  vulkan: "Vulkan",
};
// Native status/events may already contain the rendered user message. Trust
// only the exact catalogue and its finite set of closed backend expansions.
const details = new Set([
  ...catalog.details,
  ...Object.values(catalog.messages),
  ...Object.values(catalog.templates).flatMap((template) =>
    Object.values(backendLabels).map((label) =>
      template.replace("{backend}", label),
    ),
  ),
]);
const legacyPrefixes: [string, NativeErrorCode][] = [
  ["операция уже выполняется: ", "busy"],
  ["операция отменена: ", "cancelled"],
  ["аудио ошибка: ", "audio"],
  ["ошибка whisper/stt: ", "stt"],
  ["ошибка LLM: ", "llm"],
  ["ошибка вставки текста: ", "injection"],
  ["ошибка ввода-вывода: ", "io"],
  ["ошибка JSON: ", "json"],
  ["ошибка конфигурации: ", "config"],
  ["внутренняя ошибка: ", "internal"],
];

function record(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

function knownCode(value: unknown): value is NativeErrorCode {
  return typeof value === "string" && Object.hasOwn(catalog.messages, value);
}

function trustedDetail(value: unknown): string | undefined {
  if (typeof value !== "string" || value.length > 1200) return;
  const trimmed = value.trim();
  return details.has(trimmed) ? trimmed : undefined;
}

function legacy(value: string, fallback: NativeErrorCode) {
  if (value.length > 5000) return { code: fallback, params: {} };
  let message = value.trim();
  let code = fallback;
  for (let pass = 0; pass < 3; pass++) {
    const prefix = legacyPrefixes.find(([prefix]) =>
      message.startsWith(prefix),
    );
    if (!prefix) break;
    code = prefix[1];
    message = message.slice(prefix[0].length).trim();
  }
  // Compatibility with old native versions; the matched detail is never shown.
  if (message.startsWith("Whisper model is not selected"))
    code = "model_not_selected";
  else if (message.startsWith("model file not found:")) code = "model_missing";
  else if (message.includes("backend is not packaged in this release"))
    code = "backend_unavailable";
  else if (message === "модель не загружена") code = "model_not_loaded";
  const detail = trustedDetail(message);
  return { code, params: detail ? { detail } : {} };
}

function decode(error: unknown, fallback: NativeErrorCode) {
  if (
    typeof error === "string" &&
    error.length <= 5000 &&
    error.trim().startsWith("{")
  ) {
    try {
      const parsed: unknown = JSON.parse(error);
      if (record(parsed)) error = parsed;
    } catch {
      // A malformed legacy rejection is handled as an untrusted string.
    }
  }
  const object = record(error);
  if (object && knownCode(object.code)) {
    const source = record(object.params);
    const params: NativeErrorParams = {};
    const detail = trustedDetail(source?.detail ?? object.message);
    if (detail) params.detail = detail;
    if (
      typeof source?.backend === "string" &&
      Object.hasOwn(backendLabels, source.backend)
    )
      params.backend = source.backend as RecognitionBackend;
    return { code: object.code, params };
  }
  const message = typeof error === "string" ? error : object?.message;
  return typeof message === "string"
    ? legacy(message, fallback)
    : { code: fallback, params: {} as NativeErrorParams };
}

function message(code: NativeErrorCode, params: NativeErrorParams): string {
  if (params.detail) return params.detail;
  if (params.backend && Object.hasOwn(catalog.templates, code)) {
    const template = catalog.templates[code as keyof typeof catalog.templates];
    return template.replace("{backend}", backendLabels[params.backend]);
  }
  return errorMessage(code);
}

export class NativeIpcError extends Error {
  readonly code: NativeErrorCode;
  readonly params: NativeErrorParams;

  constructor(error: unknown, fallback: NativeErrorCode = "internal") {
    const decoded = decode(error, fallback);
    super(message(decoded.code, decoded.params));
    this.name = "NativeIpcError";
    this.code = decoded.code;
    this.params = decoded.params;
  }
}

export function nativeErrorMessage(
  error: unknown,
  fallback: NativeErrorCode = "internal",
): string {
  return new NativeIpcError(error, fallback).message;
}

function field(value: unknown, name: string, code: NativeErrorCode) {
  const object = record(value);
  if (!object || typeof object[name] !== "string" || !object[name])
    return value;
  return { ...object, [name]: nativeErrorMessage(object[name], code) };
}

function serviceSnapshot(value: unknown) {
  const object = record(field(value, "error", "io"));
  if (!object) return value;
  const result = { ...object };
  for (const key of ["history", "queue"]) {
    const section = record(object[key]);
    if (!section || !Array.isArray(section.jobs)) continue;
    result[key] = {
      ...section,
      jobs: section.jobs.map((job) => {
        const item = record(job);
        return item?.error
          ? { ...item, error: field(item.error, "message", "stt") }
          : job;
      }),
    };
  }
  return result;
}

/** Embedded status failures are successful IPC responses, so normalize them too. */
export function normalizeNativeResult(
  command: string,
  value: unknown,
): unknown {
  if (command === "get_stt_readiness" || command === "get_stt_health")
    return field(value, "message", "stt");
  if (command === "get_local_transcription_service_snapshot")
    return serviceSnapshot(value);
  if (command === "get_pending_dictation") return field(value, "error", "stt");
  if (
    typeof __FONO_EXPERIMENTAL_LIVE__ !== "undefined" &&
    __FONO_EXPERIMENTAL_LIVE__ &&
    command === "get_live_dictation"
  )
    return field(value, "warning", "stt");
  return value;
}

/** Legacy error events and V1 envelopes share the same Russian UI mapping. */
export function normalizeNativeEvent(channel: string, value: unknown): unknown {
  if (channel === "error") return nativeErrorMessage(value);
  if (channel === "pending-dictation") return field(value, "error", "stt");
  if (
    typeof __FONO_EXPERIMENTAL_LIVE__ !== "undefined" &&
    __FONO_EXPERIMENTAL_LIVE__ &&
    channel === "dictation-live"
  )
    return field(value, "warning", "stt");
  const envelope = record(value);
  if (channel === "backend-event-v1" && envelope?.kind === "error") {
    const payload = record(envelope.payload);
    return payload
      ? {
          ...envelope,
          payload: { ...payload, message: nativeErrorMessage(payload) },
        }
      : value;
  }
  if (channel === "backend-event-v1" && envelope?.kind === "stt_readiness") {
    const payload = record(envelope.payload);
    return payload
      ? {
          ...envelope,
          payload: {
            ...payload,
            readiness: field(payload.readiness, "message", "stt"),
          },
        }
      : value;
  }
  return value;
}
