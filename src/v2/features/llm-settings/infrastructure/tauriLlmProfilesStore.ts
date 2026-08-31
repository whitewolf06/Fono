import { ipc } from "@/lib/ipc";
import type { Settings } from "@/lib/types";
import type { LlmProfilesStore } from "../application/useLlmProfiles";

export function createTauriLlmProfilesStore(): LlmProfilesStore {
  return {
    load: () => ipc.getSettings(),
    save: (settings: Settings) => ipc.saveSettings(settings),
    test: (profileId) => ipc.testLlmProfile(profileId),
  };
}
