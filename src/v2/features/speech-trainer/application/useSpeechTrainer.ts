import { useCallback, useEffect, useRef, useState } from "react";
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
  saveTrainerEnabled(enabled: boolean): Promise<void>;
  clearHistory(): Promise<void>;
  setSessionAnalyticsIncluded(id: string, included: boolean): Promise<void>;
  subscribe(handler: () => void): () => void;
}

export function useSpeechTrainer(store: SpeechTrainerStore) {
  const [period, setPeriod] = useState<SpeechTrainerPeriod>(30);
  const [entries, setEntries] = useState<DictationHistoryEntry[]>([]);
  const [report, setReport] = useState<SpeechPeriodReport | null>(null);
  const [analyticsEnabled, setAnalyticsEnabledState] = useState(false);
  const [trainerEnabled, setTrainerEnabledState] = useState(false);
  const [trainerToggleError, setTrainerToggleError] = useState<string | null>(null);
  const [isUpdatingTrainer, setIsUpdatingTrainer] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const hasLoadedRef = useRef(false);

  const refresh = useCallback(
    async (manual = false) => {
      if (manual) setIsRefreshing(true);
      if (!hasLoadedRef.current) setIsLoading(true);
      const { from, to } = getSpeechTrainerPeriodBounds(period);

      try {
        const [nextEntries, nextReport, settings] = await Promise.all([
          store.loadHistory(),
          store.loadReport(from, to),
          store.loadSettings(),
        ]);
        setEntries(nextEntries);
        setReport(nextReport);
        setAnalyticsEnabledState(settings.analytics_enabled);
        setTrainerEnabledState(
          settings.analytics_enabled && settings.speech_trainer_enabled,
        );
        setError(null);
      } catch (reason) {
        setError(
          reason instanceof Error
            ? reason.message
            : "Не удалось загрузить отчёт.",
        );
      } finally {
        hasLoadedRef.current = true;
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

  const setTrainerEnabled = async (enabled: boolean) => {
    setTrainerToggleError(null);
    setIsUpdatingTrainer(true);
    try {
      await store.saveTrainerEnabled(enabled);
      await refresh();
    } catch (reason) {
      setTrainerToggleError(
        reason instanceof Error
          ? reason.message
          : "Не удалось изменить состояние речевого тренера.",
      );
    } finally {
      setIsUpdatingTrainer(false);
    }
  };

  return {
    analyticsEnabled,
    entries,
    error,
    isLoading,
    isRefreshing,
    isUpdatingTrainer,
    period,
    report,
    setPeriod,
    clearHistory,
    setSessionAnalyticsIncluded,
    setTrainerEnabled,
    trainerEnabled,
    trainerToggleError,
    refresh: () => refresh(true),
  };
}
