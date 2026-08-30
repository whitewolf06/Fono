import type { ServiceSnapshot } from "../domain/serviceMonitor";

export interface ServiceRuntime {
  getSnapshot(): Promise<ServiceSnapshot>;
  cancelJob(id: string): Promise<void>;
  copyText(text: string): Promise<void>;
}
