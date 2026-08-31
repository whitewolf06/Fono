import { useCallback, useEffect, useState } from "react";
import type {
  DictationHistoryEntry,
  Settings,
  SpeechPeriodReport,
} from "@/lib/types";
import {
  getSpeechTrainerPeriodBounds,
  type SpeechTrainerPeriod,
} from "../domain/speechTrainer";

export interface SpeechTrainerStore {
  loadHistory(): Promise<DictationHistoryEntry[]>;
  loadReport(from: string, to: string): Promise<SpeechPeriodReport>;
  loadSettings(): Promise<Settings>;
  clearHistory(): Promise<void>;
  setSessionAnalyticsIncluded(id: string, included: boolean): Promise<void>;
  subscribe(handler: () => void): () => void;
}

export function useSpeechTrainer(store: SpeechTrainerStore) {
  const [period, setPeriod] = useState<SpeechTrainerPeriod>(30);
  const [entries, setEntries] = useState<DictationHistoryEntry[]>([]);
  const [report, setReport] = useState<SpeechPeriodReport | null>(null);
  const [analyticsEnabled, setAnalyticsEnabled] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isRefreshing, setIsRefreshing] = useState(false);

  const refresh = useCallback(
    async (manual = false) => {
      if (manual) setIsRefreshing(true);
      if (!manual) setIsLoading(true);
      const { from, to } = getSpeechTrainerPeriodBounds(period);

      try {
        const [nextEntries, nextReport, settings] = await Promise.all([
          store.loadHistory(),
          store.loadReport(from, to),
          store.loadSettings(),
        ]);
        setEntries(nextEntries);
        setReport(nextReport);
        setAnalyticsEnabled(settings.analytics_enabled);
        setError(null);
      } catch (reason) {
        setError(
          reason instanceof Error
            ? reason.message
            : "Не удалось загрузить отчёт.",
        );
      } finally {
        setIsLoading(false);
        setIsRefreshing(false);
      }
    },
    [period, store],
  );

  useEffect(() => {
    void refresh();
    return store.subscribe(() => void refresh());
  }, [refresh, store]);

  const clearHistory = async () => {
    await store.clearHistory();
    await refresh();
  };

  const setSessionAnalyticsIncluded = async (id: string, included: boolean) => {
    await store.setSessionAnalyticsIncluded(id, included);
    await refresh();
  };

  return {
    analyticsEnabled,
    entries,
    error,
    isLoading,
    isRefreshing,
    period,
    report,
    setPeriod,
    clearHistory,
    setSessionAnalyticsIncluded,
    refresh: () => refresh(true),
  };
}
