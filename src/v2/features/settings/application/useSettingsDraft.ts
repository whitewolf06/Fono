import { useEffect, useState } from "react";
import {
  loadCollapsedSettingsSections,
  saveCollapsedSettingsSections,
} from "../infrastructure/settingsUiPreferences";

export type SettingsSection =
  | "general"
  | "audio"
  | "activation"
  | "processing"
  | "privacy"
  | "overlay"
  | "advanced";

export type SettingsStatus = "idle" | "checking" | "ready" | "error";

export interface SettingsDraft {
  language: string;
  insertionMode: "sendinput" | "clipboard";
  autostart: boolean;
  hotkey: string;
  microphone: string;
  microphoneOptions: string[];
  recognitionModel: string;
  recognitionModelOptions: string[];
  acceleration: "auto" | "cuda" | "vulkan" | "cpu";
  wakeWordEnabled: boolean;
  wakePhrase: string;
  wakeSensitivity: number;
  silenceDelay: number;
  processingEnabled: boolean;
  processingMode: "clean" | "format";
  historyEnabled: boolean;
  analyticsEnabled: boolean;
  analyticsRetentionDays: number;
  overlayVisible: boolean;
  overlayScale: number;
  overlayOpacity: number;
  overlayMiniMode: boolean;
  verboseLogging: boolean;
}

export interface SettingsStatusDetail {
  state: SettingsStatus;
  message: string;
}

export interface ProcessingPreview {
  sourceText: string;
  processedText: string;
  displayedText: "source" | "processed";
  fallbackActive: boolean;
}

export interface SettingsDraftStore {
  load(): Promise<SettingsDraft>;
  save(draft: SettingsDraft): Promise<void>;
  testMicrophone?(): Promise<{ peak: number; rms: number }>;
  downloadWhisperModel?(model: string): Promise<void>;
  testLmStudio?(): Promise<string>;
}

const initialDraft: SettingsDraft = {
  language: "auto",
  insertionMode: "sendinput",
  autostart: true,
  hotkey: "Ctrl + Alt + F",
  microphone: "Microphone Array (Realtek)",
  microphoneOptions: ["Default system device"],
  recognitionModel: "Whisper Small",
  recognitionModelOptions: ["Whisper Small"],
  acceleration: "auto",
  wakeWordEnabled: true,
  wakePhrase: "okay fun",
  wakeSensitivity: 72,
  silenceDelay: 2,
  processingEnabled: true,
  processingMode: "clean",
  historyEnabled: true,
  analyticsEnabled: false,
  analyticsRetentionDays: 30,
  overlayVisible: true,
  overlayScale: 100,
  overlayOpacity: 92,
  overlayMiniMode: false,
  verboseLogging: false,
};

const initialProcessingPreview: ProcessingPreview = {
  sourceText: "так вот я хотел бы отправить письмо сегодня",
  processedText: "Так вот, я хотел бы отправить письмо сегодня.",
  displayedText: "processed",
  fallbackActive: false,
};

export function useSettingsDraft(store?: SettingsDraftStore) {
  const [draft, setDraft] = useState<SettingsDraft>(initialDraft);
  const [advancedWakeOpen, setAdvancedWakeOpen] = useState(false);
  const [collapsedSections, setCollapsedSections] = useState<SettingsSection[]>(
    loadCollapsedSettingsSections,
  );
  const [saveState, setSaveState] = useState<
    "idle" | "saving" | "saved" | "error"
  >("idle");
  const [microphoneStatus, setMicrophoneStatus] =
    useState<SettingsStatusDetail>({
      state: "ready",
      message: "Микрофон доступен для записи.",
    });
  const [whisperStatus, setWhisperStatus] = useState<SettingsStatusDetail>({
    state: "idle",
    message: "Модель распознавания ещё не синхронизирована с приложением.",
  });
  const [wakeWordStatus, setWakeWordStatus] = useState<SettingsStatusDetail>({
    state: "ready",
    message: "Sherpa-ONNX ожидает ключевую фразу.",
  });
  const [overlayStatus, setOverlayStatus] = useState<SettingsStatusDetail>({
    state: "idle",
    message: "Тестовый показ ещё не запускался.",
  });
  const [lmStudioStatus, setLmStudioStatus] = useState<SettingsStatusDetail>({
    state: "idle",
    message: "Подключение ещё не проверялось.",
  });
  const [effectiveAcceleration, setEffectiveAcceleration] = useState("Vulkan");
  const [hasMicrophoneSample, setHasMicrophoneSample] = useState(false);
  const [processingPreview, setProcessingPreview] = useState(
    initialProcessingPreview,
  );

  useEffect(() => {
    saveCollapsedSettingsSections(collapsedSections);
  }, [collapsedSections]);

  useEffect(() => {
    if (!store) return;

    let active = true;
    void store.load().then(
      (nextDraft) => {
        if (!active) return;
        setDraft(nextDraft);
        setEffectiveAcceleration(resolveEffectiveAcceleration(nextDraft.acceleration));
        setWhisperStatus({
          state: "ready",
          message: `Выбрана ${nextDraft.recognitionModel}.`,
        });
      },
      () => {
        if (active) setSaveState("error");
      },
    );

    return () => {
      active = false;
    };
  }, [store]);

  const update = <Key extends keyof SettingsDraft>(
    key: Key,
    value: SettingsDraft[Key],
  ) => {
    setDraft((current) => ({ ...current, [key]: value }));
    setSaveState("idle");

    if (key === "acceleration") {
      setEffectiveAcceleration(resolveEffectiveAcceleration(value));
    }
  };

  const toggleCollapsedSection = (section: SettingsSection) => {
    setCollapsedSections((current) =>
      current.includes(section)
        ? current.filter((item) => item !== section)
        : [...current, section],
    );
  };

  const saveSettings = async () => {
    setSaveState("saving");

    if (store) {
      try {
        await store.save(draft);
        const savedDraft = await store.load();
        setDraft(savedDraft);
        setEffectiveAcceleration(
          resolveEffectiveAcceleration(savedDraft.acceleration),
        );
        setWhisperStatus({
          state: "ready",
          message: `Сохранена ${savedDraft.recognitionModel}.`,
        });
        setSaveState("saved");
      } catch {
        setSaveState("error");
      }
      return;
    }

    window.setTimeout(() => setSaveState("saved"), 550);
  };

  const testMicrophone = () => {
    if (store?.testMicrophone) {
      setMicrophoneStatus({
        state: "checking",
        message: "Testing microphone…",
      });
      void store.testMicrophone().then(
        ({ peak, rms }) => {
          setHasMicrophoneSample(true);
          setMicrophoneStatus({
            state: "ready",
            message: `Microphone ready: RMS ${Math.round(rms * 100)}%, peak ${Math.round(peak * 100)}%.`,
          });
        },
        (error: unknown) =>
          setMicrophoneStatus({ state: "error", message: errorMessage(error) }),
      );
      return;
    }
    setMicrophoneStatus({
      state: "checking",
      message: "Записываю короткий образец…",
    });
    window.setTimeout(() => {
      setHasMicrophoneSample(true);
      setMicrophoneStatus({
        state: "ready",
        message: "Образец записан: средний уровень 34%.",
      });
    }, 700);
  };

  const playMicrophoneSample = () => {
    setMicrophoneStatus({
      state: "ready",
      message: "В UI v2 образец готов к прослушиванию через runtime.",
    });
  };

  const reloadWhisperModel = () => {
    if (store?.downloadWhisperModel) {
      setWhisperStatus({
        state: "checking",
        message: "Downloading Whisper model…",
      });
      void store.downloadWhisperModel(draft.recognitionModel).then(
        () =>
          setWhisperStatus({
            state: "ready",
            message: `${draft.recognitionModel} is downloaded and ready.`,
          }),
        (error: unknown) =>
          setWhisperStatus({ state: "error", message: errorMessage(error) }),
      );
      return;
    }
    setWhisperStatus({
      state: "checking",
      message: "Загружаю Whisper-модель…",
    });
    window.setTimeout(() => {
      setWhisperStatus({
        state: "ready",
        message: `${draft.recognitionModel} загружена и готова к распознаванию.`,
      });
    }, 650);
  };

  const testWakeWord = () => {
    if (!draft.wakeWordEnabled) {
      setWakeWordStatus({
        state: "error",
        message: "Включите wake word, чтобы проверить ключевую фразу.",
      });
      return;
    }

    setWakeWordStatus({
      state: "checking",
      message: "Проверяю ключевую фразу…",
    });
    window.setTimeout(() => {
      setWakeWordStatus({
        state: "ready",
        message: `Фраза «${draft.wakePhrase}» передана в тест wake word.`,
      });
    }, 600);
  };

  const showOverlayTest = () => {
    if (!draft.overlayVisible) {
      setOverlayStatus({
        state: "error",
        message: "Включите overlay, чтобы показать тестовое окно.",
      });
      return;
    }

    setOverlayStatus({
      state: "ready",
      message: "Тестовый сценарий подготовлен для отдельного overlay-окна.",
    });
  };

  const testLmStudio = () => {
    if (store?.testLmStudio) {
      setLmStudioStatus({ state: "checking", message: "Checking LM Studio…" });
      void store.testLmStudio().then(
        (response) =>
          setLmStudioStatus({
            state: "ready",
            message: `LM Studio is available: ${response}`,
          }),
        (error: unknown) =>
          setLmStudioStatus({ state: "error", message: errorMessage(error) }),
      );
      return;
    }
    setLmStudioStatus({ state: "checking", message: "Проверяю подключение…" });
    window.setTimeout(() => {
      setLmStudioStatus({
        state: "error",
        message:
          "UI v2 ещё не подключён к runtime LM Studio. Исходный текст останется без обработки.",
      });
    }, 550);
  };

  const restoreOriginalTranscript = () => {
    setProcessingPreview((current) => ({
      ...current,
      displayedText: "source",
      fallbackActive: true,
    }));
  };

  return {
    advancedWakeOpen,
    collapsedSections,
    draft,
    effectiveAcceleration,
    hasMicrophoneSample,
    lmStudioStatus,
    microphoneStatus,
    overlayStatus,
    playMicrophoneSample,
    processingPreview,
    reloadWhisperModel,
    restoreOriginalTranscript,
    saveSettings,
    saveState,
    setAdvancedWakeOpen,
    showOverlayTest,
    testLmStudio,
    testMicrophone,
    testWakeWord,
    toggleCollapsedSection,
    update,
    wakeWordStatus,
    whisperStatus,
  };
}

function resolveEffectiveAcceleration(
  value: SettingsDraft[keyof SettingsDraft],
) {
  if (value === "cuda") return "CUDA";
  if (value === "vulkan") return "Vulkan";
  if (value === "cpu") return "CPU";
  return "Vulkan";
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : "Native check failed.";
}
