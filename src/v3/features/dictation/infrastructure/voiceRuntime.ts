import { isTauri } from "@tauri-apps/api/core";
import { ipc, onPipelineStateChange } from "@/lib/ipc";
import type { VoiceOverview, VoicePhase } from "../domain/voice";
import type { VoiceRuntime } from "../application/voiceRuntime";

const demoText =
  "Fono распознаёт речь локально и помогает быстро перенести мысль в любое приложение.";

function createMockRuntime(): VoiceRuntime {
  let phase: VoicePhase = "idle";
  let wakeWordEnabled = true;
  let history = [
    {
      id: "demo-1",
      text: "Сегодня хочется сосредоточиться на главном и не терять хорошие идеи.",
      createdAt: new Date(Date.now() - 48 * 60_000).toISOString(),
    },
    {
      id: "demo-2",
      text: "Созвон с командой перенесём на завтра после обеда.",
      createdAt: new Date(Date.now() - 2 * 60 * 60_000).toISOString(),
    },
  ];
  const listeners = new Set<(value: VoicePhase) => void>();
  const emit = (value: VoicePhase) => {
    phase = value;
    listeners.forEach((listener) => listener(value));
  };

  return {
    demo: true,
    load: async () => ({
      phase,
      hotkey: "Ctrl + Space",
      language: "Русский",
      model: "Whisper Small",
      wakeWordEnabled,
      wakeWord: "Эй, Fono",
      history,
      version: __FONO_FRONTEND_BUILD__.version,
      error: null,
    }),
    start: async () => emit("listening"),
    stop: async () => {
      emit("transcribing");
      await new Promise((resolve) => window.setTimeout(resolve, 850));
      history = [
        {
          id: `demo-${Date.now()}`,
          text: demoText,
          createdAt: new Date().toISOString(),
        },
        ...history,
      ];
      emit("idle");
    },
    toggleWakeWord: async () => {
      wakeWordEnabled = !wakeWordEnabled;
    },
    copyText: async (text) => {
      await navigator.clipboard.writeText(text);
    },
    subscribePhase: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}

function createTauriRuntime(): VoiceRuntime {
  return {
    demo: false,
    load: async (): Promise<VoiceOverview> => {
      const [phase, settings, history, build] = await Promise.all([
        ipc.getPipelineState(),
        ipc.getSettings(),
        ipc.getDictationHistory(),
        ipc.getBuildInfo(),
      ]);

      return {
        phase,
        hotkey: settings.hotkey,
        language:
          settings.language === "auto"
            ? "Авто"
            : settings.language.toUpperCase(),
        model: settings.whisper_model_path
          ? (settings.whisper_model_path.split(/[\\/]/).at(-1) ?? "Whisper")
          : "Модель не выбрана",
        wakeWordEnabled: settings.wake_word_enabled,
        wakeWord: settings.wake_word,
        history: history.slice(0, 12).map((entry) => ({
          id: entry.id,
          text: entry.text,
          createdAt: entry.created_at,
        })),
        version: build.version,
        error: null,
      };
    },
    start: () => ipc.startDictation(),
    stop: async () => {
      await ipc.stopDictation();
    },
    toggleWakeWord: async () => {
      const settings = await ipc.getSettings();
      if (settings.wake_word_enabled) await ipc.disableWakeWord();
      else await ipc.enableWakeWord();
    },
    copyText: (text) => ipc.copyDictationText(text),
    subscribePhase: (listener) => {
      let active = true;
      let unlisten: (() => void) | undefined;
      void onPipelineStateChange((phase) => {
        if (active) listener(phase);
      }).then((dispose) => {
        if (active) unlisten = dispose;
        else dispose();
      });
      return () => {
        active = false;
        unlisten?.();
      };
    },
  };
}

export function createVoiceRuntime(): VoiceRuntime {
  return isTauri() ? createTauriRuntime() : createMockRuntime();
}
