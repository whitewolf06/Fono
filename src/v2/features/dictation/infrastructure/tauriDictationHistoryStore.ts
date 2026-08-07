import { ipc } from "@/lib/ipc";
import type { DictationHistoryEntry } from "@/lib/types";

export interface DictationHistoryStore {
  load(): Promise<DictationHistoryEntry[]>;
  clear(): Promise<void>;
  reinsert(text: string): Promise<void>;
}

export function createTauriDictationHistoryStore(): DictationHistoryStore {
  return {
    load: () => ipc.getDictationHistory(),
    clear: () => ipc.clearDictationHistory(),
    reinsert: (text) => ipc.reinsertDictation(text),
  };
}
