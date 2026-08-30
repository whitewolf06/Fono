import { useCallback, useEffect, useState } from "react";
import type { ServiceRuntime } from "./serviceRuntime";
import type { ServiceSnapshot } from "../domain/serviceMonitor";

const REFRESH_INTERVAL_MS = 1_000;

export function useServiceMonitor(runtime: ServiceRuntime) {
  const [snapshot, setSnapshot] = useState<ServiceSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isRefreshing, setIsRefreshing] = useState(false);

  const refresh = useCallback(async () => {
    setIsRefreshing(true);
    try {
      const next = await runtime.getSnapshot();
      setSnapshot(next);
      setError(null);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setIsRefreshing(false);
    }
  }, [runtime]);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), REFRESH_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const cancelJob = async (id: string) => {
    await runtime.cancelJob(id);
    await refresh();
  };

  const copyText = (text: string) => runtime.copyText(text);

  return { snapshot, error, isRefreshing, refresh, cancelJob, copyText };
}
