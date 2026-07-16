import type { DictationRuntime } from "../application/dictationRuntime";
import type {
  DictationSettingsSummary,
  DictationSnapshot,
  ReadinessSnapshot,
} from "@/v2/shared/domain/pipeline";

const initialSnapshot: DictationSnapshot = {
  phase: "idle",
  mode: "dictation",
  transcript: "",
  hotkey: "Ctrl + Space",
  language: "Русский",
  error: null,
};

const initialReadiness: ReadinessSnapshot = {
  microphone: "ready",
  model: "ready",
  wakeWord: "active",
};

const settingsSummary: DictationSettingsSummary = {
  recognitionModel: "Whisper Small",
  accelerator: "Vulkan",
  postProcessing: "Пунктуация · Light",
};

export function createMockDictationRuntime(): DictationRuntime {
  let snapshot = initialSnapshot;
  let readiness = initialReadiness;
  const listeners = new Set<(next: DictationSnapshot) => void>();
  let timer: number | undefined;

  const emit = (next: DictationSnapshot) => {
    snapshot = next;
    listeners.forEach((listener) => listener(snapshot));
  };

  const clearTimer = () => {
    if (timer !== undefined) window.clearTimeout(timer);
    timer = undefined;
  };

  return {
    getSnapshot: async () => snapshot,
    getReadiness: async () => readiness,
    getSettingsSummary: async () => settingsSummary,
    toggleWakeWord: async () => {
      readiness = {
        ...readiness,
        wakeWord: readiness.wakeWord === "active" ? "disabled" : "active",
      };
    },
    start: async () => {
      clearTimer();
      emit({ ...snapshot, phase: "listening", transcript: "", error: null });
    },
    stop: async () => {
      clearTimer();
      emit({
        ...snapshot,
        phase: "transcribing",
        transcript:
          "Fono превращает речь в текст локально — быстро, приватно и без лишних шагов.",
      });
      timer = window.setTimeout(() => {
        emit({ ...snapshot, phase: "processing" });
        timer = window.setTimeout(
          () => emit({ ...snapshot, phase: "idle" }),
          900,
        );
      }, 900);
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}
