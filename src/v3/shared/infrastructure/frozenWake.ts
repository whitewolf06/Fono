import type { WakePort } from "../domain/wake";
import { ensureWakeAvailable } from "../domain/wakeAvailability";

/** Compatibility port for builds that contain no detector or wake IPC. */
export function createFrozenWake(): WakePort {
  const unavailable = async () => ensureWakeAvailable();
  return {
    load: async () => {},
    cancel: async () => {},
    download: unavailable,
    test: async () => {
      ensureWakeAvailable();
      return "";
    },
    begin: unavailable,
    record: unavailable,
    beginValidation: unavailable,
    validate: unavailable,
  };
}
