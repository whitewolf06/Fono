export type ServiceJobState =
  | "queued"
  | "preparing"
  | "transcribing"
  | "completed"
  | "failed"
  | "cancelled";

export interface ServiceJobResult {
  text: string;
  detectedLanguage: string | null;
  audioSeconds: number | null;
  transcribeSeconds: number | null;
  model: string;
  backend: string | null;
}

export interface ServiceJob {
  id: string;
  state: ServiceJobState;
  createdAtMs: number;
  startedAtMs: number | null;
  finishedAtMs: number | null;
  result: ServiceJobResult | null;
  error: string | null;
}

export interface ServiceQueueSnapshot {
  capacity: number;
  queued: number;
  preparing: number;
  transcribing: number;
  completed: number;
  failed: number;
  cancelled: number;
  jobs: ServiceJob[];
}

export interface ServiceSnapshot {
  address: string;
  protocolVersion: number;
  model: string;
  acceleration: string;
  queue: ServiceQueueSnapshot;
}

export function isActiveServiceJob(job: ServiceJob): boolean {
  return ["queued", "preparing", "transcribing"].includes(job.state);
}

export function serviceJobLabel(state: ServiceJobState): string {
  return {
    queued: "В очереди",
    preparing: "Подготовка",
    transcribing: "Распознаётся",
    completed: "Готово",
    failed: "Ошибка",
    cancelled: "Отменено",
  }[state];
}
