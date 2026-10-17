export type CudaPackagePhase =
  | "unavailable"
  | "not_installed"
  | "checking"
  | "downloading"
  | "verifying"
  | "activating"
  | "cancelling"
  | "installed"
  | "error"
  | "cancelled";

export interface CudaPackageStatus {
  phase: CudaPackagePhase;
  operationId: string | null;
  downloadedBytes: number;
  totalBytes: number | null;
  installedRevision: number | null;
  message: string;
  nvidiaDetected?: boolean | null;
}

export interface CudaPackagesPort {
  state: CudaPackageStatus;
  refresh(): Promise<void>;
  install(): Promise<void>;
  cancel(): Promise<void>;
  dispose(): void;
}

export function cudaPackageBusy(phase: CudaPackagePhase): boolean {
  return [
    "checking",
    "downloading",
    "verifying",
    "activating",
    "cancelling",
  ].includes(phase);
}

export function cudaPackageProgress(status: CudaPackageStatus): number | null {
  if (!status.totalBytes || status.totalBytes <= 0) return null;
  return Math.min(
    100,
    Math.max(0, Math.floor((100 * status.downloadedBytes) / status.totalBytes)),
  );
}
