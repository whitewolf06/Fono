import type { Settings } from "@/lib/types";

/**
 * The trainer state is separate from privacy consent: disabling it only pauses
 * collection and analysis of future sessions. Existing local data remains.
 */
export function withSpeechTrainerEnabled(
  settings: Settings,
  enabled: boolean,
): Settings {
  return { ...settings, speech_trainer_enabled: enabled };
}
