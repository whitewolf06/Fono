import { ipc } from "@/lib/ipc";
import type { WakeProfileValidationStore } from "../application/useWakeProfileValidation";

export function createTauriWakeProfileValidationStore(): WakeProfileValidationStore {
  return {
    getStatus: () => ipc.getWakeProfileValidationStatus(),
    start: () => ipc.startWakeProfileValidation(),
    record: (kind) => ipc.recordWakeProfileValidationSample(kind),
  };
}
