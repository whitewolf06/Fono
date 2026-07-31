import { ipc, onError, onPipelineStateChange } from "@/lib/ipc";
import type { Settings } from "@/lib/types";
import type {
  DictationSettingsSummary,
  DictationSnapshot,
  ReadinessSnapshot,
} from "@/v2/shared/domain/pipeline";
import type { DictationRuntime } from "../application/dictationRuntime";

export function createTauriDictationRuntime(): DictationRuntime {
  let transcript = "";
  let latestSnapshot: DictationSnapshot = createSnapshot("idle", null);
  const listeners = new Set<(snapshot: DictationSnapshot) => void>();

  const emit = (snapshot: DictationSnapshot) => {
    latestSnapshot = snapshot;
    listeners.forEach((listener) => listener(snapshot));
  };

  const refreshSnapshot = async () => {
    const [phase, settings] = await Promise.all([
      ipc.getPipelineState(),
      ipc.getSettings(),
    ]);
    const next = createSnapshot(phase, settings);
    latestSnapshot = next;
    return next;
  };

  return {
    getSnapshot: refreshSnapshot,
    getReadiness: () => getReadiness(),
    getSettingsSummary: async () => toSettingsSummary(await ipc.getSettings()),
    toggleWakeWord: async () => {
      const settings = await ipc.getSettings();

      if (settings.wake_word_enabled) {
        await ipc.disableWakeWord();
      } else {
        await ipc.enableWakeWord();
      }
    },
    start: async () => {
      transcript = "";
      await ipc.startDictation();
    },
    stop: async () => {
      const result = await ipc.stopDictation();
      transcript = result.text;
      const next = await refreshSnapshot();
      emit(next);
    },
    subscribe: (listener) => {
      let active = true;
      let disposePipeline: (() => void) | undefined;
      let disposeError: (() => void) | undefined;

      listeners.add(listener);

      void onPipelineStateChange(async (phase) => {
        const settings = await ipc.getSettings();
        if (active) emit(createSnapshot(phase, settings));
      }).then((unlisten) => {
        if (active) disposePipeline = unlisten;
        else unlisten();
      });

      void onError((message) => {
        if (active) {
          emit({ ...latestSnapshot, phase: "error", error: message });
        }
      }).then((unlisten) => {
        if (active) disposeError = unlisten;
        else unlisten();
      });

      return () => {
        active = false;
        listeners.delete(listener);
        disposePipeline?.();
        disposeError?.();
      };
    },
  };

  function createSnapshot(
    phase: DictationSnapshot["phase"],
    settings: Settings | null,
  ): DictationSnapshot {
    return {
      phase,
      mode: "dictation",
      transcript,
      hotkey: settings?.hotkey ?? "Ctrl + Space",
      language: settings?.language ?? "auto",
      error: null,
    };
  }
}

async function getReadiness(): Promise<ReadinessSnapshot> {
  const [settings, devices] = await Promise.all([
    ipc.getSettings(),
    ipc.listAudioDevices(),
  ]);

  return {
    microphone: devices.length ? "ready" : "attention",
    model: settings.whisper_model_path ? "ready" : "missing",
    wakeWord: settings.wake_word_enabled ? "active" : "disabled",
  };
}

function toSettingsSummary(settings: Settings): DictationSettingsSummary {
  return {
    recognitionModel: modelLabel(settings.whisper_model_path),
    accelerator:
      settings.acceleration === "auto"
        ? "Авто"
        : settings.acceleration.toUpperCase(),
    postProcessing: postProcessingLabel(settings),
  };
}

function modelLabel(path: string | null): string {
  if (!path) return "Модель не выбрана";

  return (
    path
      .split(/[\\/]/)
      .at(-1)
      ?.replace(/^ggml-/, "")
      .replace(/\.bin$/, "")
      .replace(/^./, (letter) => letter.toUpperCase()) ?? "Whisper"
  );
}

function postProcessingLabel(settings: Settings): string {
  if (settings.ai_mode === "off") return "Выключена";
  if (settings.ai_mode === "format") return "Форматирование";
  if (settings.ai_mode === "command") return "Команды";
  return "Пунктуация · Light";
}
