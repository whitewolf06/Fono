import { message as uiMessage } from "./messages";
import type { ProcessingPreset, TranslationLanguage } from "./processing";
import type { OverlayProcessingChoice } from "./processing";
export interface ApplicationSessionPreferences extends OverlayProcessingChoice {
  sessionId: number;
  profileName: string | null;
  processingEnabled: boolean;
  translationEnabled: boolean;
  recognitionLanguage: string;
  recognition: import("./recognition").OverlayRecognitionCapability;
}
export interface ApplicationProfile {
  id: string;
  name: string;
  enabled: boolean;
  executables: string[];
  processingEnabled: boolean | null;
  preset: ProcessingPreset | null;
  translationEnabled: boolean | null;
  targetLanguage: TranslationLanguage | null;
  language: string | null;
  injectionMode: "clipboard" | "sendinput" | null;
}
export function cloneApplicationProfiles(
  profiles: ApplicationProfile[],
): ApplicationProfile[] {
  return profiles.map((p) => ({ ...p, executables: [...p.executables] }));
}
export function newApplicationProfile(
  existing: ApplicationProfile[],
): ApplicationProfile {
  let number = 1;
  while (existing.some((p) => p.id === `app-${number}`)) number++;
  return {
    id: `app-${number}`,
    name: "",
    enabled: true,
    executables: [],
    processingEnabled: null,
    preset: null,
    translationEnabled: null,
    targetLanguage: null,
    language: null,
    injectionMode: null,
  };
}
export function validateApplicationProfiles(
  profiles: ApplicationProfile[],
): string | null {
  if (!Array.isArray(profiles) || profiles.length > 32)
    return uiMessage("ui.cabc411394");
  const ids = new Set<string>(),
    executables = new Set<string>();
  for (const p of profiles) {
    if (
      !/^[a-zA-Z0-9_-]{1,64}$/.test(p.id) ||
      ids.has(p.id) ||
      !p.name.trim() ||
      [...p.name].length > 64 ||
      [...p.name].some(
        (c) =>
          c.charCodeAt(0) < 32 ||
          (c.charCodeAt(0) >= 127 && c.charCodeAt(0) <= 159),
      )
    )
      return uiMessage("ui.5066a6d359");
    ids.add(p.id);
    if (
      typeof p.enabled !== "boolean" ||
      !p.executables.length ||
      p.executables.length > 16
    )
      return uiMessage("ui.e25317c837");
    for (const exe of p.executables) {
      if (
        exe.length > 128 ||
        exe.length <= 4 ||
        exe.trim() !== exe ||
        !/^[a-zA-Z0-9 ._-]+\.exe$/i.test(exe)
      )
        return uiMessage("ui.d0e05fcb84");
      if (executables.has(exe.toLowerCase())) return uiMessage("ui.4ce600b830");
      executables.add(exe.toLowerCase());
    }
    if (
      ![null, true, false].includes(p.processingEnabled) ||
      ![null, true, false].includes(p.translationEnabled) ||
      ![null, "raw", "clean", "format", "task", "formal"].includes(p.preset)
    )
      return uiMessage("ui.608756c9b1");
    if (
      p.targetLanguage !== null &&
      !["en", "ru", "de", "fr", "es"].includes(p.targetLanguage)
    )
      return uiMessage("ui.ddf776c712");
    if (p.language !== null && !["auto", "ru", "en"].includes(p.language))
      return uiMessage("ui.486b2c993c");
    if (![null, "clipboard", "sendinput"].includes(p.injectionMode))
      return uiMessage("ui.8f392f4f6a");
  }
  return null;
}
