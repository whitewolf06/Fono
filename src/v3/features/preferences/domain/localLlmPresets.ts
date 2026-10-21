import type { ConnectionProfile } from "../../../shared/domain/contracts";
export type LocalLlmPreset = "ollama" | "lmstudio";
export const localLlmPresets = {
  ollama: {
    name: "Ollama",
    provider: "ollama",
    location: "local",
    url: "http://127.0.0.1:11434/v1",
  },
  lmstudio: {
    name: "LM Studio",
    provider: "lmstudio",
    location: "local",
    url: "http://127.0.0.1:1234/v1",
  },
} as const;
export function localPresetFor(
  profile: ConnectionProfile,
): LocalLlmPreset | null {
  if (profile.location !== "local") return null;
  return (
    (Object.keys(localLlmPresets) as LocalLlmPreset[]).find(
      (key) =>
        profile.provider === localLlmPresets[key].provider &&
        profile.url.replace(/\/$/, "") === localLlmPresets[key].url,
    ) ?? null
  );
}
export function applyLocalPreset(
  profile: ConnectionProfile,
  preset: LocalLlmPreset,
): ConnectionProfile {
  // Retargeting a saved profile could retain a vault credential for its ID.
  // Templates create a new draft only; existing profiles keep their normal editor.
  if (profile.id) throw new Error("Local templates require a new profile");
  return {
    ...profile,
    ...localLlmPresets[preset],
    model: "",
    apiKey: "",
    hasApiKey: false,
  };
}
