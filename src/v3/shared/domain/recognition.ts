import { message as uiMessage } from "./messages";
export type RecognitionLanguage = "ru" | "en" | "auto";
export interface OverlayRecognitionCapability {
  options: RecognitionLanguage[];
  fixedLanguage: string | null;
}
export const recognitionOptions = [
  { value: "ru", label: "RU" },
  { value: "en", label: "EN" },
  {
    value: "auto",
    get label() {
      return uiMessage("ui.8dca2f7831");
    },
  },
] as const;

/** Browser preview only. Native capability always comes from the session snapshot. */
export function previewRecognitionCapability(
  model: string,
): OverlayRecognitionCapability {
  if (model.startsWith("giga_am:")) return { options: [], fixedLanguage: "ru" };
  if (model.startsWith("parakeet:"))
    return { options: [], fixedLanguage: "auto" };
  if (!model || model === "custom" || model.includes(":"))
    return { options: [], fixedLanguage: null };
  return { options: ["ru", "en", "auto"], fixedLanguage: null };
}
