import type { Preferences } from "../domain/contracts";
import { defaults } from "../../features/preferences/domain/preferences";
import { availableDictationMode } from "../domain/dictationMode";
const storageKey = "fono-v3-demo-preferences-v1";
// Persist demo preferences only. Transcripts, tokens and instructions never enter browser storage.
const persistedKeys: (keyof Preferences)[] = [
  "dictationMode",
  "microphone",
  "model",
  "language",
  "acceleration",
  "dictionaryEnabled",
  "wakeEnabled",
  "wakePhrase",
  "wakeLanguage",
  "silenceMs",
  "hotkey",
  "hotkeyMode",
  "commandHotkey",
  "processingEnabled",
  "processingMode",
  "processingTrigger",
  "processingTranslation",
  "profile",
  "processingModel",
  "autostart",
  "insertion",
  "overlayEnabled",
  "overlayCompact",
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
    if (!raw || typeof raw !== "object") return { ...defaults };
    const safe: Record<string, unknown> = {};
    for (const key of persistedKeys) {
      const value = (raw as Record<string, unknown>)[key];
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
    if (!["ru", "en"].includes(String(safe.wakeLanguage)))
      safe.wakeLanguage = defaults.wakeLanguage;
    for (const [key, values] of Object.entries({
      hotkeyMode: ["hold", "toggle"],
      processingMode: ["clean", "format", "task", "formal"],
      processingTrigger: ["automatic", "manual"],
      processingTranslation: ["none", "en", "ru", "de", "fr", "es"],
    })) {
      if (!values.includes(String(safe[key])))
        safe[key] = defaults[key as keyof Preferences];
    }
    return { ...defaults, ...safe, dictionaryEntries: [] };
  } catch {
    return { ...defaults };
  }
}
export function persistPreferences(prefs: Preferences): void {
  const safe = Object.fromEntries(
    persistedKeys.map((key) => [key, prefs[key]]),
  );
  safe.dictationMode = availableDictationMode(prefs.dictationMode);
  // Dictionary terms remain in memory; only the optional switch is persisted.
  try {
    localStorage.setItem(storageKey, JSON.stringify(safe));
  } catch {
    throw new Error(
      "Не удалось сохранить настройки в браузере. Проверьте доступ к хранилищу.",
    );
  }
}
export async function copyToClipboard(text: string): Promise<void> {
  if (!navigator.clipboard)
    throw new Error(
      "Буфер обмена недоступен. Выделите текст и скопируйте вручную.",
    );
  await navigator.clipboard.writeText(text);
}
export function playDemoSample(): Promise<void> {
  return new Promise((resolve, reject) => {
    if (!("speechSynthesis" in window)) {
      reject(new Error("Прослушивание не поддерживается этим браузером."));
      return;
    }
    const sample = new SpeechSynthesisUtterance(
      "Это демонстрационный образец голоса Fono.",
    );
    sample.lang = "ru-RU";
    sample.onend = () => resolve();
    sample.onerror = () =>
      reject(new Error("Не удалось воспроизвести образец."));
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
