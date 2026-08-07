import { ipc } from "@/lib/ipc";
import type { DictationHistoryEntry } from "@/lib/types";

export interface DictationHistoryStore {
  load(): Promise<DictationHistoryEntry[]>;
  clear(): Promise<void>;
  delete(id: string): Promise<void>;
  copy(text: string): Promise<void>;
  reinsert(text: string): Promise<void>;
}

export function createTauriDictationHistoryStore(): DictationHistoryStore {
  return {
    load: () => ipc.getDictationHistory(),
    clear: () => ipc.clearDictationHistory(),
    delete: (id) => ipc.deleteDictationHistoryEntry(id),
    copy: (text) => ipc.copyDictationText(text),
    reinsert: (text) => ipc.reinsertDictation(text),
  };
}
