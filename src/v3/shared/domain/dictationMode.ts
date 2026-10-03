import type { DictationMode } from "./contracts";

// Keep experimental live dictation unavailable until its quality is approved.
export const LIVE_DICTATION_ENABLED = false;

export function availableDictationMode(
  mode: DictationMode | undefined,
): DictationMode {
  return LIVE_DICTATION_ENABLED && mode === "live" ? "live" : "standard";
}
