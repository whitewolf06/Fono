export type GpuModelResidency = "resident" | "adaptive";

export interface GpuMemoryStatus {
  mode: GpuModelResidency;
  residency: "unloaded" | "resident" | "active";
  monitoring: "disabled" | "monitoring" | "unsupported";
  message: string;
  usedBytes?: number;
  totalBytes?: number;
}

export function availableGpuModelResidency(value: unknown): GpuModelResidency {
  return value === "adaptive" ? "adaptive" : "resident";
}

export function gpuMemoryUsage(status?: GpuMemoryStatus): string | null {
  if (
    status?.monitoring !== "monitoring" ||
    status.usedBytes == null ||
    status.totalBytes == null ||
    !Number.isFinite(status.usedBytes) ||
    !Number.isFinite(status.totalBytes) ||
    status.usedBytes < 0 ||
    status.totalBytes <= 0
  )
    return null;
  const gib = (value: number) => (value / 1024 ** 3).toFixed(1);
  return `${gib(status.usedBytes)} / ${gib(status.totalBytes)} ГБ видеопамяти занято`;
}
