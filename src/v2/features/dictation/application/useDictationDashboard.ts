import { useEffect, useState } from "react";
import type { DictationRuntime } from "./dictationRuntime";
import type { DictationSnapshot, ReadinessSnapshot } from "@/v2/shared/domain/pipeline";

export function useDictationDashboard(runtime: DictationRuntime) {
  const [snapshot, setSnapshot] = useState<DictationSnapshot | null>(null);
  const [readiness, setReadiness] = useState<ReadinessSnapshot | null>(null);

  useEffect(() => {
    let active = true;
    runtime.getSnapshot().then((next) => active && setSnapshot(next));
    runtime.getReadiness().then((next) => active && setReadiness(next));
    return runtime.subscribe((next) => active && setSnapshot(next));
  }, [runtime]);

  return {
    snapshot,
    readiness,
    start: () => runtime.start(),
    stop: () => runtime.stop(),
  };
}
