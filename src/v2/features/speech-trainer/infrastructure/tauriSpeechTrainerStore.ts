import { ipc, onSpeechAnalysisChanged } from "@/lib/ipc";
import type { SpeechTrainerStore } from "../application/useSpeechTrainer";

export function createTauriSpeechTrainerStore(): SpeechTrainerStore {
  return {
    loadHistory: () => ipc.getDictationHistory(),
    loadReport: (from, to) => ipc.getSpeechPeriodReport(from, to),
    loadSettings: () => ipc.getSettings(),
    clearHistory: () => ipc.clearDictationHistory(),
    setSessionAnalyticsIncluded: async (id, included) => {
      await ipc.setDictationHistoryEntryAnalyticsIncluded(id, included);
    },
    subscribe: (handler) => {
      let disposed = false;
      let unlisten: (() => void) | null = null;

      void onSpeechAnalysisChanged(handler).then((nextUnlisten) => {
        if (disposed) nextUnlisten();
        else unlisten = nextUnlisten;
      });

      return () => {
        disposed = true;
        unlisten?.();
      };
    },
  };
}
