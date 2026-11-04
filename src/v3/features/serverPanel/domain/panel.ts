export type PanelTab = "status" | "models" | "queue" | "docs";
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
