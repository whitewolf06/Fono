export type UpdatePhase =
  | "not_configured"
  | "idle"
  | "checking"
  | "up_to_date"
  | "available"
  | "downloading"
  | "installing"
  | "error";

export interface UpdateStatus {
  phase: UpdatePhase;
  currentVersion: string;
  nextVersion?: string | null;
  downloadedBytes: number;
  totalBytes?: number | null;
  message: string;
  checksEnabled: boolean;
}

export interface UpdatesPort {
  state: UpdateStatus;
  refresh(): Promise<void>;
  check(): Promise<void>;
  install(): Promise<void>;
  cancel(): Promise<void>;
  setChecksEnabled(enabled: boolean): Promise<void>;
  dispose(): void;
}

export function updateBusy(phase: UpdatePhase) {
  return ["checking", "downloading", "installing"].includes(phase);
}

export function downloadPercent(status: UpdateStatus): number | null {
  if (!status.totalBytes || status.totalBytes <= 0) return null;
  return Math.min(
    100,
    Math.max(0, Math.round((100 * status.downloadedBytes) / status.totalBytes)),
  );
}
