import { reactive } from "vue";
import type { SettingsPort } from "../../../shared/domain/contracts";
import type { LocalLlmPreset } from "../domain/localLlmPresets";
export function createLocalModelsLookup(
  port: Pick<SettingsPort, "listLocalModels">,
) {
  let generation = 0;
  const state = reactive({
    status: "idle" as
      "idle" | "checking" | "available" | "empty" | "unavailable",
    models: [] as string[],
  });
  function reset() {
    generation++;
    state.status = "idle";
    state.models = [];
  }
  async function check(preset: LocalLlmPreset) {
    const request = ++generation;
    state.status = "checking";
    state.models = [];
    try {
      const models = await port.listLocalModels(preset);
      if (request !== generation) return;
      state.models = [
        ...new Set(models.map((id) => id.trim()).filter(Boolean)),
      ];
      state.status = state.models.length ? "available" : "empty";
    } catch {
      if (request === generation) state.status = "unavailable";
    }
  }
  return { state, check, reset };
}
