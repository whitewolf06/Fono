import { useCallback, useEffect, useState } from "react";
import type { ServiceRuntime } from "./serviceRuntime";
import type { ServiceSnapshot } from "../domain/serviceMonitor";

export function useServiceMonitor(runtime: ServiceRuntime) {
  const [snapshot, setSnapshot] = useState<ServiceSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isRefreshing, setIsRefreshing] = useState(false);

  const loadSnapshot = useCallback(
    async (manual = false) => {
      if (manual) setIsRefreshing(true);
      try {
        const next = await runtime.getSnapshot();
        setSnapshot(next);
        setError(null);
      } catch (reason) {
        setError(String(reason));
      } finally {
        if (manual) setIsRefreshing(false);
      }
    },
    [runtime],
  );

  useEffect(() => {
    void loadSnapshot();
    return runtime.subscribe(() => void loadSnapshot());
  }, [loadSnapshot, runtime]);

  const cancelJob = async (id: string) => {
    await runtime.cancelJob(id);
    await loadSnapshot();
  };

  const clearHistory = async () => {
    await runtime.clearHistory();
    await loadSnapshot();
  };

  const copyText = (text: string) => runtime.copyText(text);
  const copyApiToken = () => runtime.copyApiToken();

  return {
    snapshot,
    error,
    isRefreshing,
    refresh: () => loadSnapshot(true),
    cancelJob,
    clearHistory,
    copyText,
    copyApiToken,
  };
}
