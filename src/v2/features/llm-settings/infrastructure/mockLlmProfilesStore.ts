import { DEFAULT_SETTINGS, type Settings } from "@/lib/types";
import type { LlmProfilesStore } from "../application/useLlmProfiles";

let settings: Settings = structuredClone(DEFAULT_SETTINGS);

export function createMockLlmProfilesStore(): LlmProfilesStore {
  return {
    load: async () => structuredClone(settings),
    save: async (next) => {
      settings = structuredClone(next);
    },
    test: async (profileId) =>
      `Mock LLM profile ${profileId} active, model: qwen3`,
  };
}
