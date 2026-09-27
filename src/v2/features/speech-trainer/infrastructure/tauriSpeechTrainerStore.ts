import { ipc, onSettingsChange, onSpeechAnalysisChanged } from "@/lib/ipc";
import type { SpeechTrainerStore } from "../application/useSpeechTrainer";
import { withSpeechTrainerEnabled } from "../domain/speechTrainerSettings";

export function createTauriSpeechTrainerStore(): SpeechTrainerStore {
  return {
    loadHistory: () => ipc.getDictationHistory(),
    loadReport: (from, to) => ipc.getSpeechPeriodReport(from, to),
    loadSettings: () => ipc.getSettings(),
    saveTrainerEnabled: async (enabled) => {
      const settings = await ipc.getSettings();
      await ipc.saveSettings(withSpeechTrainerEnabled(settings, enabled));
    },
    clearHistory: () => ipc.clearDictationHistory(),
    setSessionAnalyticsIncluded: async (id, included) => {
      await ipc.setDictationHistoryEntryAnalyticsIncluded(id, included);
    },
    subscribe: (handler) => {
      let disposed = false;
      const unlisten = new Set<() => void>();

      const subscribe = (listen: Promise<() => void>) => {
        void listen.then((nextUnlisten) => {
          if (disposed) nextUnlisten();
          else unlisten.add(nextUnlisten);
        });
      };

      subscribe(onSpeechAnalysisChanged(handler));
      subscribe(onSettingsChange(() => handler()));

      return () => {
        disposed = true;
        unlisten.forEach((dispose) => dispose());
      };
    },
  };
}
