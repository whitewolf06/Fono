import { useCallback, useEffect, useState } from "react";
import type { Settings } from "@/lib/types";

export interface LlmProfilesStore {
  load(): Promise<Settings>;
  save(settings: Settings): Promise<void>;
  test(profileId: string): Promise<string>;
}

export function useLlmProfiles(store: LlmProfilesStore) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const refresh = useCallback(async () => {
    setIsLoading(true);
    try {
      setSettings(await store.load());
      setError(null);
    } catch (reason) {
      setError(
        reason instanceof Error
          ? reason.message
          : "Не удалось загрузить LLM-настройки.",
      );
    } finally {
      setIsLoading(false);
    }
  }, [store]);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  const save = async (next: Settings) => {
    setIsSaving(true);
    try {
      await store.save(next);
      setSettings(await store.load());
      setMessage("Настройки LLM сохранены.");
      setError(null);
    } catch (reason) {
      setError(
        reason instanceof Error
          ? reason.message
          : "Не удалось сохранить LLM-настройки.",
      );
    } finally {
      setIsSaving(false);
    }
  };
  const test = async (profileId: string) => {
    try {
      setMessage(await store.test(profileId));
      setError(null);
    } catch (reason) {
      setError(
        reason instanceof Error ? reason.message : "Проверка LLM не удалась.",
      );
    }
  };
  return { settings, isLoading, isSaving, message, error, refresh, save, test };
}
