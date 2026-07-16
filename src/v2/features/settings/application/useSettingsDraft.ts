import { useState } from "react";

export type SettingsSection =
  "general" | "audio" | "activation" | "processing" | "overlay" | "advanced";

export interface SettingsDraft {
  language: string;
  insertionMode: "sendinput" | "clipboard";
  autostart: boolean;
  hotkey: string;
  microphone: string;
  recognitionModel: string;
  acceleration: "auto" | "cuda" | "vulkan" | "cpu";
  wakeWordEnabled: boolean;
  wakePhrase: string;
  wakeSensitivity: number;
  silenceDelay: number;
  processingEnabled: boolean;
  processingMode: "clean" | "format";
  overlayVisible: boolean;
  overlayScale: number;
  overlayOpacity: number;
  overlayMiniMode: boolean;
  verboseLogging: boolean;
}

const initialDraft: SettingsDraft = {
  language: "auto",
  insertionMode: "sendinput",
  autostart: true,
  hotkey: "Ctrl + Alt + F",
  microphone: "Microphone Array (Realtek)",
  recognitionModel: "Whisper Small",
  acceleration: "auto",
  wakeWordEnabled: true,
  wakePhrase: "okay fun",
  wakeSensitivity: 72,
  silenceDelay: 2,
  processingEnabled: true,
  processingMode: "clean",
  overlayVisible: true,
  overlayScale: 100,
  overlayOpacity: 92,
  overlayMiniMode: false,
  verboseLogging: false,
};

export function useSettingsDraft() {
  const [draft, setDraft] = useState<SettingsDraft>(initialDraft);
  const [advancedWakeOpen, setAdvancedWakeOpen] = useState(false);

  const update = <Key extends keyof SettingsDraft>(
    key: Key,
    value: SettingsDraft[Key],
  ) => {
    setDraft((current) => ({ ...current, [key]: value }));
  };

  return {
    advancedWakeOpen,
    draft,
    setAdvancedWakeOpen,
    update,
  };
}
