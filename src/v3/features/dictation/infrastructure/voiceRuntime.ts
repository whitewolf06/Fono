import { isTauri } from "@tauri-apps/api/core";
import { ipc, onPipelineStateChange } from "@/lib/ipc";
import type { DeviceInfo, Settings } from "@/lib/types";
import type { VoiceOverview, VoicePhase, VoiceTools } from "../domain/voice";
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
  const start = async () => emit("listening");
  const stop = async () => {
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
  };
  const onHotkey = (event: KeyboardEvent) => {
    if (!event.ctrlKey || event.code !== "Space" || event.repeat) return;
    event.preventDefault();
    if (phase === "idle") void start();
    else if (phase === "listening") void stop();
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
      tools: {
        microphone: { enabled: true, value: "Системный микрофон" },
        wakeWord: { enabled: wakeWordEnabled, value: "Эй, Fono" },
        postProcessing: { enabled: true, value: "GPT-4o Mini" },
        recognition: { enabled: true, value: "Whisper Small" },
      },
      services: {
        api: { enabled: true, address: "127.0.0.1:17832", queued: 0 },
        trainer: { enabled: false, dictationCount: history.length },
      },
      history,
      version: __FONO_FRONTEND_BUILD__.version,
      error: null,
    }),
    start,
    stop,
    toggleWakeWord: async () => {
      wakeWordEnabled = !wakeWordEnabled;
    },
    copyText: async (text) => {
      await navigator.clipboard.writeText(text);
    },
    subscribePhase: (listener) => {
      listeners.add(listener);
      if (listeners.size === 1) document.addEventListener("keydown", onHotkey);
      return () => {
        listeners.delete(listener);
        if (listeners.size === 0)
          document.removeEventListener("keydown", onHotkey);
      };
    },
  };
}

function createTauriRuntime(): VoiceRuntime {
  return {
    demo: false,
    load: async (): Promise<VoiceOverview> => {
      const [phase, settings, history, build, devices, service] =
        await Promise.all([
          ipc.getPipelineState(),
          ipc.getSettings(),
          ipc.getDictationHistory(),
          ipc.getBuildInfo(),
          ipc.listAudioDevices().catch(() => null),
          ipc.getLocalTranscriptionServiceSnapshot().catch(() => null),
        ]);

      const model = recognitionModelLabel(settings.whisper_model_path);

      return {
        phase,
        hotkey: settings.hotkey,
        language:
          settings.language === "auto"
            ? "Авто"
            : settings.language.toUpperCase(),
        model,
        wakeWordEnabled: settings.wake_word_enabled,
        wakeWord: settings.wake_word,
        tools: toVoiceTools(settings, devices, model),
        services: {
          api: {
            enabled: service !== null,
            address: service?.address ?? "Недоступен",
            queued: service?.queue.queued ?? 0,
          },
          trainer: {
            enabled:
              settings.analytics_enabled && settings.speech_trainer_enabled,
            dictationCount: history.length,
          },
        },
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

function toVoiceTools(
  settings: Settings,
  devices: DeviceInfo[] | null,
  recognitionModel: string,
): VoiceTools {
  const microphone = settings.audio_device_id
    ? devices?.find((device) => device.id === settings.audio_device_id)
    : devices?.find((device) => device.is_default);
  const profile = settings.llm_profiles.find(
    (candidate) => candidate.id === settings.text_correction_llm.profile_id,
  );
  const postProcessingModel = profile
    ? settings.text_correction_llm.model?.trim() ||
      profile.model?.trim() ||
      "Автовыбор модели"
    : settings.llm_model?.trim() || "Автовыбор модели";

  return {
    microphone: {
      enabled: Boolean(microphone),
      value:
        microphone?.name ??
        (devices === null
          ? "Не удалось проверить"
          : settings.audio_device_id
            ? "Устройство недоступно"
            : "Системный микрофон недоступен"),
    },
    wakeWord: {
      enabled: settings.wake_word_enabled,
      value: settings.wake_word.trim() || "Фраза не задана",
    },
    postProcessing: {
      enabled: settings.ai_mode !== "off",
      value: postProcessingModel,
    },
    recognition: {
      enabled: Boolean(settings.whisper_model_path),
      value: recognitionModel,
    },
  };
}

function recognitionModelLabel(path: string | null): string {
  const filename = path?.split(/[\\/]/).at(-1);
  if (!filename) return "Модель не выбрана";

  const known: Record<string, string> = {
    "ggml-tiny.bin": "Whisper Tiny",
    "ggml-base.bin": "Whisper Base",
    "ggml-small.bin": "Whisper Small",
    "ggml-medium.bin": "Whisper Medium",
    "ggml-large-v3.bin": "Whisper Large v3",
    "ggml-large-v3-turbo.bin": "Whisper Large v3 Turbo",
  };
  return known[filename.toLowerCase()] ?? filename;
}

export function createVoiceRuntime(): VoiceRuntime {
  return isTauri() ? createTauriRuntime() : createMockRuntime();
}
