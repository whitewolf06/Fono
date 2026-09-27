import { ipc } from "@/lib/ipc";
import type { WakeCalibrationStore } from "../application/useWakeCalibration";

export function createTauriWakeCalibrationStore(): WakeCalibrationStore {
  return {
    getStatus: () => ipc.getWakeCalibrationStatus(),
    start: () => ipc.startWakeCalibration(),
    recordNext: () => ipc.recordWakeCalibrationSample(),
    cancel: () => ipc.cancelWakeCalibration(),
  };
}
