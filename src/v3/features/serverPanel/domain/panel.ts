export type PanelTab = "status" | "models" | "queue" | "clients" | "docs";
export interface ServerClient {
  name: string;
  scope: "recognize";
  created_at: string;
}
export type ClientRequest =
  { method: "GET" } | { method: "POST" | "DELETE"; name: string };
export interface PanelResponse {
  status: number;
  data: unknown;
  text: string;
  requestId: string;
  retryAfter: number;
  elapsedMs: number;
}
export interface PanelAudio {
  name: string;
  size: number;
  source: unknown;
}
export interface PanelRequest {
  method: "GET" | "POST";
  path: string;
  model?: string;
  language?: string;
  format?: string;
  timestamps?: string;
}
export interface PanelPort {
  requestClients(
    request: ClientRequest,
    token: string,
    signal: AbortSignal,
  ): Promise<PanelResponse>;
  copyText(value: string): Promise<void>;
  request(
    request: PanelRequest,
    token: string,
    signal: AbortSignal,
    audio?: PanelAudio,
  ): Promise<PanelResponse>;
  chooseAudio(): Promise<PanelAudio | null>;
  schedule(callback: () => void, delayMs: number): () => void;
  now(): number;
  connection: string;
}
export function validClientName(name: string): boolean {
  return (
    typeof name === "string" && /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(name)
  );
}
/** A single dotenv-safe origin, never a URL inferred from the browser tunnel. */
export function apiBaseUrl(value: string): string | null {
  if (
    !/^https?:\/\/(?:[A-Za-z0-9][A-Za-z0-9._-]*|\[[A-Fa-f0-9:]+\])(?::[0-9]{1,5})?\/?$/.test(
      value,
    )
  )
    return null;
  try {
    const url = new URL(value);
    if (
      !["http:", "https:"].includes(url.protocol) ||
      url.username ||
      url.password ||
      url.search ||
      url.hash ||
      url.pathname !== "/" ||
      ["0.0.0.0", "[::]"].includes(url.hostname) ||
      url.port === "0" ||
      !/^https?:\/\//.test(value)
    )
      return null;
    return url.origin;
  } catch {
    return null;
  }
}
export function clientEnvironment(url: string, token: string): string | null {
  const origin = apiBaseUrl(url);
  return origin && /^fono_[a-f0-9]{64}$/.test(token)
    ? `FONO_HOST=${origin}\nFONO_TOKEN=${token}`
    : null;
}
export interface ServerCapabilities {
  version: string;
  api_version: number;
  worker_protocol: number;
  max_audio_seconds: number;
  max_upload_bytes: number;
  devices: {
    id: string;
    kind: string;
    name?: string;
    memory_total_mb?: number;
  }[];
  features: { openai_compatible: boolean; metrics: boolean };
}
export interface ServerHealth {
  status: string;
  models: { id: string; state: string; device: string }[];
  queue: { interactive: number; batch: number };
}
export interface ServerModel {
  id: string;
  fono: {
    engine: string;
    languages: string[];
    size_bytes: number;
    state: string;
    disk_state: string;
    device: string;
    default: boolean;
  };
}
export interface ApiOperation {
  method: string;
  path: string;
  summary: string;
  description: string;
  schema: unknown;
  runnable: boolean;
}
export const readablePaths = [
  "/healthz",
  "/v1/health",
  "/v1/capabilities",
  "/v1/models",
  "/openapi.json",
] as const;
export function isRunnable(method: string, path: string): boolean {
  return method === "GET"
    ? readablePaths.some((value) => value === path)
    : method === "POST" &&
        ["/v1/recognize", "/v1/audio/transcriptions"].includes(path);
}
export function operationsFrom(value: unknown): ApiOperation[] {
  const paths = (value as { paths?: Record<string, unknown> })?.paths;
  if (!paths || typeof paths !== "object") return [];
  return Object.entries(paths).flatMap(([path, entry]) =>
    Object.entries((entry ?? {}) as Record<string, unknown>)
      .filter(([method]) =>
        ["get", "post", "delete", "put", "patch"].includes(method),
      )
      .map(([method, schema]) => {
        const detail = schema as { summary?: string; description?: string };
        return {
          method: method.toUpperCase(),
          path,
          summary: detail.summary ?? "",
          description: detail.description ?? "",
          schema,
          runnable: isRunnable(method.toUpperCase(), path),
        };
      }),
  );
}
export function stateLabel(value: string): string {
  const labels: Record<string, string> = {
    ready: "Готов",
    ok: "Готов",
    starting: "Запускается",
    loading: "Загружается",
    busy: "Распознаёт",
    failed: "Ошибка",
    unloaded: "Выгружена",
    retiring: "Освобождается",
    present_unverified: "Есть на диске · хэш не проверен",
    invalid: "Повреждена",
  };
  return labels[value] ?? value;
}
export function formatBytes(value: number): string {
  return value >= 1024 ** 3
    ? `${(value / 1024 ** 3).toFixed(1)} ГиБ`
    : `${(value / 1024 ** 2).toFixed(1)} МиБ`;
}
