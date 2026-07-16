import { useEffect, useState } from "react";
import type { DictationRuntime } from "./dictationRuntime";
import type {
  DictationSettingsSummary,
  DictationSnapshot,
  ReadinessSnapshot,
} from "@/v2/shared/domain/pipeline";

export function useDictationDashboard(runtime: DictationRuntime) {
  const [snapshot, setSnapshot] = useState<DictationSnapshot | null>(null);
  const [readiness, setReadiness] = useState<ReadinessSnapshot | null>(null);
  const [settingsSummary, setSettingsSummary] =
    useState<DictationSettingsSummary | null>(null);

  useEffect(() => {
    let active = true;
    runtime.getSnapshot().then((next) => active && setSnapshot(next));
    runtime.getReadiness().then((next) => active && setReadiness(next));
    runtime
      .getSettingsSummary()
      .then((next) => active && setSettingsSummary(next));
    return runtime.subscribe((next) => active && setSnapshot(next));
  }, [runtime]);

  return {
    snapshot,
    readiness,
    settingsSummary,
    start: () => runtime.start(),
    stop: () => runtime.stop(),
    toggleWakeWord: async () => {
      await runtime.toggleWakeWord();
      const next = await runtime.getReadiness();
      setReadiness(next);
    },
  };
}
