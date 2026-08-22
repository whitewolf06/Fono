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
    const showRuntimeError = (error: unknown) => {
      if (!active) return;
      setSnapshot((current) => ({
        phase: "error",
        mode: current?.mode ?? "dictation",
        transcript: current?.transcript ?? "",
        hotkey: current?.hotkey ?? "Ctrl+Space",
        language: current?.language ?? "auto",
        error: String(error),
      }));
    };
    runtime
      .getSnapshot()
      .then((next) => active && setSnapshot(next), showRuntimeError);
    runtime
      .getReadiness()
      .then((next) => active && setReadiness(next), showRuntimeError);
    runtime
      .getSettingsSummary()
      .then((next) => active && setSettingsSummary(next), showRuntimeError);

    const stopSnapshot = runtime.subscribe((next) => active && setSnapshot(next));
    const stopSettings = runtime.subscribeSettings?.((next) => {
      if (!active) return;
      setSettingsSummary(next);
      void runtime
        .getReadiness()
        .then((readiness) => active && setReadiness(readiness), showRuntimeError);
    });

    return () => {
      stopSnapshot();
      stopSettings?.();
    };
  }, [runtime]);

  return {
    snapshot,
    readiness,
    settingsSummary,
    start: async () => {
      try {
        await runtime.start();
      } catch (error) {
        setSnapshot((current) =>
          current
            ? { ...current, phase: "error", error: String(error) }
            : current,
        );
      }
    },
    stop: async () => {
      try {
        await runtime.stop();
      } catch (error) {
        setSnapshot((current) =>
          current
            ? { ...current, phase: "error", error: String(error) }
            : current,
        );
      }
    },
    toggleWakeWord: async () => {
      try {
        await runtime.toggleWakeWord();
        const next = await runtime.getReadiness();
        setReadiness(next);
      } catch (error) {
        setSnapshot((current) =>
          current
            ? { ...current, phase: "error", error: String(error) }
            : current,
        );
      }
    },
  };
}
