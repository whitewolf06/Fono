import { message as uiMessage } from "../domain/messages";
import { validToggleAutoStop } from "../domain/toggleAutoStop";
import { recordingSoundsVolume } from "../domain/recordingSounds";
import type { Preferences } from "../domain/contracts";
import { defaults } from "../../features/preferences/domain/preferences";
import { availableDictationMode } from "../domain/dictationMode";
import { availableWakeEnabled } from "../domain/wakeAvailability";
import { clonePreferences } from "../../features/preferences/domain/processingPrompts";
import { PROJECT_SITE_URL } from "../domain/project";
const storageKey = "fono-v3-demo-preferences-v1";
export function showStartupFailure(): void {
  const root = document.getElementById("app");
  if (root) root.textContent = uiMessage("ui.32c3b49aea");
}
export async function openProjectSite(): Promise<void> {
  window.open(PROJECT_SITE_URL, "_blank", "noopener,noreferrer");
}
// Persist demo preferences only. Transcripts, tokens and instructions never enter browser storage.
const persistedKeys: (keyof Preferences)[] = [
  "uiLanguage",
  "pretranscriptionEnabled",
  "dictationMode",
  "microphone",
  "model",
  "language",
  "acceleration",
  "whisperDecoding",
  "whisperThreads",
  "gpuModelResidency",
  "dictionaryEnabled",
  "dictionaryLlmEnabled",
  "wakeEnabled",
  "wakePhrase",
  "wakeLanguage",
  "silenceMs",
  "hotkey",
  "dictationTrigger",
  "selectionCaptureEnabled",
  "selectionHotkey",
  "selectionClipboardFallback",
  "selectionAutoInsert",
  "selectionHistoryEnabled",
  "hotkeyMode",
  "toggleAutoStopMs",
  "recordingSoundsEnabled",
  "recordingSoundsVolume",
  "commandHotkey",
  "processingEnabled",
  "processingMode",
  "processingTrigger",
  "processingTranslation",
  "processingTranslationEnabled",
  "profile",
  "processingModel",
  "autostart",
  "insertion",
  "overlayEnabled",
  "overlayCompact",
  "overlayQuickProcessing",
  "overlayScale",
  "overlayOpacity",
  "overlayPosition",
  "historyEnabled",
  "trainerEnabled",
  "analyticsConsent",
  "retentionDays",
  "trainerAiEnabled",
  "trainerProfile",
  "trainerModel",
  "trainerScope",
  "cloudConsent",
  "serviceEnabled",
];
export function readPreferences(): Preferences {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(storageKey) || "{}");
    if (!raw || typeof raw !== "object") return clonePreferences(defaults);
    const safe: Record<string, unknown> = {};
    for (const key of persistedKeys) {
      const value = (raw as Record<string, unknown>)[key];
      if (key === "whisperThreads") {
        safe[key] =
          typeof value === "number" &&
          Number.isInteger(value) &&
          value >= 1 &&
          value <= 8
            ? value
            : null;
        continue;
      }
      if (
        typeof value === typeof defaults[key] &&
        (typeof value !== "string" || value.length < 100)
      )
        safe[key] = value;
    }
    if (!["local", "cloud"].includes(String(safe.profile)))
      safe.profile = defaults.profile;
    if (!["local", "cloud"].includes(String(safe.trainerProfile)))
      safe.trainerProfile = defaults.trainerProfile;
    safe.dictationMode = availableDictationMode(
      safe.dictationMode as Preferences["dictationMode"],
    );
    safe.wakeEnabled = availableWakeEnabled(Boolean(safe.wakeEnabled));
    if (!validToggleAutoStop(Number(safe.toggleAutoStopMs)))
      safe.toggleAutoStopMs = 0;
    safe.recordingSoundsVolume = recordingSoundsVolume(
      safe.recordingSoundsVolume,
    );
    if (!["ru", "en"].includes(String(safe.wakeLanguage)))
      safe.wakeLanguage = defaults.wakeLanguage;
    for (const [key, values] of Object.entries({
      uiLanguage: ["auto", "ru", "en"],
      gpuModelResidency: ["resident", "adaptive"],
      whisperDecoding: ["fast", "accurate"],
      hotkeyMode: ["hold", "toggle"],
      dictationTrigger: [
        "shortcut",
        "right_alt",
        "right_ctrl",
        "caps_lock",
        "x_button1",
        "x_button2",
      ],
      processingMode: ["raw", "clean", "format", "task", "formal"],
      processingTrigger: ["automatic", "manual"],
      processingTranslation: ["none", "en", "ru", "de", "fr", "es"],
    })) {
      if (!values.includes(String(safe[key])))
        safe[key] = defaults[key as keyof Preferences];
    }
    return { ...clonePreferences(defaults), ...safe, dictionaryEntries: [] };
  } catch {
    return clonePreferences(defaults);
  }
}
export function persistPreferences(prefs: Preferences): void {
  const safe = Object.fromEntries(
    persistedKeys.map((key) => [key, prefs[key]]),
  );
  safe.dictationMode = availableDictationMode(prefs.dictationMode);
  safe.wakeEnabled = availableWakeEnabled(prefs.wakeEnabled);
  // Dictionary terms remain in memory; only the optional switch is persisted.
  try {
    localStorage.setItem(storageKey, JSON.stringify(safe));
  } catch {
    throw new Error(uiMessage("ui.d9c14e71ec"));
  }
}
export async function copyToClipboard(text: string): Promise<void> {
  if (!navigator.clipboard) throw new Error(uiMessage("ui.12ae2fb32d"));
  await navigator.clipboard.writeText(text);
}
export function playDemoSample(): Promise<void> {
  return new Promise((resolve, reject) => {
    if (!("speechSynthesis" in window)) {
      reject(new Error(uiMessage("ui.6e8bd3d830")));
      return;
    }
    const sample = new SpeechSynthesisUtterance(
      "Это демонстрационный образец голоса Fono.",
    );
    sample.lang = "ru-RU";
    sample.onend = () => resolve();
    sample.onerror = () => reject(new Error(uiMessage("ui.66b5d47e14")));
    window.speechSynthesis.speak(sample);
  });
}
export function bindShortcut(
  getHotkey: () => string,
  action: () => void,
  onRelease?: () => void,
): () => void {
  let heldKey: string | null = null;
  const listener = (event: KeyboardEvent) => {
    if (
      event.repeat ||
      (event.target instanceof HTMLElement &&
        event.target.closest(
          "input, textarea, [contenteditable], [role=dialog], [role=combobox]",
        ))
    )
      return;
    const parts = getHotkey().split(" + ");
    const key = parts.at(-1);
    if (
      event.ctrlKey === parts.includes("Ctrl") &&
      event.altKey === parts.includes("Alt") &&
      event.shiftKey === parts.includes("Shift") &&
      (key === "Space"
        ? event.code === "Space"
        : event.key.toUpperCase() === key?.toUpperCase())
    ) {
      event.preventDefault();
      heldKey = event.code || event.key;
      action();
    }
  };
  const release = (event?: KeyboardEvent) => {
    if (heldKey && (!event || (event.code || event.key) === heldKey)) {
      heldKey = null;
      onRelease?.();
    }
  };
  window.addEventListener("keydown", listener);
  window.addEventListener("keyup", release);
  const blur = () => release();
  window.addEventListener("blur", blur);
  return () => {
    window.removeEventListener("keydown", listener);
    window.removeEventListener("keyup", release);
    window.removeEventListener("blur", blur);
  };
}
export function protectUnload(isDirty: () => boolean): () => void {
  const handler = (e: BeforeUnloadEvent) => {
    if (isDirty()) {
      e.preventDefault();
      e.returnValue = "";
    }
  };
  window.addEventListener("beforeunload", handler);
  return () => window.removeEventListener("beforeunload", handler);
}
export function setupDocument(): void {
  document.documentElement.dataset.wlTheme = "graphite";
  document.documentElement.lang = "ru";
}
export function readWizardStep(): number {
  try {
    return Math.min(
      4,
      Math.max(0, Number(localStorage.getItem("fono-v3-wizard-step")) || 0),
    );
  } catch {
    return 0;
  }
}
export function saveWizardStep(step: number): void {
  try {
    localStorage.setItem("fono-v3-wizard-step", String(step));
  } catch {
    /* Demo remains usable without persistence. */
  }
}
