import { message as uiMessage } from "./messages";
import type { SpeechModel, Preferences } from "./contracts";
export type SpeechEngine = "whisper" | "giga_am" | "parakeet";
export interface SpeechModelId {
  engine: SpeechEngine;
  variant: string;
}
export function modelKey(id: SpeechModelId): string {
  return id.engine === "whisper" ? id.variant : `${id.engine}:${id.variant}`;
}
export function modelId(key: string): SpeechModelId {
  const separator = key.indexOf(":");
  if (separator < 0) return { engine: "whisper", variant: key };
  const engine = key.slice(0, separator);
  if (engine !== "giga_am" && engine !== "parakeet")
    throw new Error(uiMessage("ui.96bc41bfc5"));
  return { engine, variant: key.slice(separator + 1) };
}
export const engineNames = {
  whisper: "Whisper",
  giga_am: "GigaAM",
  parakeet: "Parakeet",
};
export function compatibleModelPreferences(
  model: SpeechModel | undefined,
  preferences: Preferences,
): Partial<Preferences> {
  if (!model || !model.engine || model.engine === "whisper") return {};
  return {
    ...(!model.languageSelection ||
    (preferences.language !== "auto" &&
      !model.languages?.includes(preferences.language))
      ? { language: "auto" }
      : {}),
    ...(!["auto", "cpu"].includes(preferences.acceleration)
      ? { acceleration: "auto" }
      : {}),
  };
}
export function validateModelPreferences(
  model: SpeechModel | undefined,
  preferences: Preferences,
): void {
  if (!model || model.status !== "installed")
    throw new Error(uiMessage("ui.a746ceacb3"));
  if (model.engine && model.engine !== "whisper") {
    if (
      preferences.language !== "auto" &&
      !model.languages?.includes(preferences.language)
    )
      throw new Error(uiMessage("ui.4c5a7d3ac6"));
    if (!["auto", "cpu"].includes(preferences.acceleration))
      throw new Error(uiMessage("ui.db88e567a8"));
  }
}
