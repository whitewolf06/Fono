import {
  computed,
  reactive,
  watch,
  onScopeDispose,
  type MaybeRefOrGetter,
  toValue,
} from "vue";
import type { Preferences } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
import { protectUnload } from "../../../shared/infrastructure/browser";
import { clonePreferences, equalPreference } from "../domain/processingPrompts";
export function useDraft(keys: MaybeRefOrGetter<(keyof Preferences)[]>) {
  const workspace = useWorkspace();
  const interaction = useInteraction();
  const draft = reactive(clonePreferences(workspace.state.preferences));
  const baseline = reactive(clonePreferences(workspace.state.preferences));
  const dirty = computed(() =>
    toValue(keys).some((key) => !equalPreference(draft[key], baseline[key])),
  );
  function reset() {
    Object.assign(draft, clonePreferences(workspace.state.preferences));
    Object.assign(baseline, clonePreferences(workspace.state.preferences));
  }
  watch(
    () => ({ ...workspace.state.preferences }),
    (next) => {
      for (const key of Object.keys(next) as (keyof Preferences)[]) {
        if (
          equalPreference(draft[key], baseline[key]) ||
          typeof next[key] === "boolean"
        )
          Object.assign(draft, { [key]: clonePreferences(next[key]) });
      }
      Object.assign(baseline, clonePreferences(next));
    },
  );
  watch(() => toValue(keys).join(), reset);
  async function save() {
    const patch = Object.fromEntries(
      toValue(keys)
        .filter((key) => !equalPreference(draft[key], baseline[key]))
        .map((key) => [key, clonePreferences(draft[key])]),
    );
    const submitted = clonePreferences(draft);
    await workspace.settings.save(patch);
    for (const key of toValue(keys)) {
      if (equalPreference(draft[key], submitted[key]))
        Object.assign(draft, {
          [key]: clonePreferences(workspace.state.preferences[key]),
        });
    }
    Object.assign(baseline, clonePreferences(workspace.state.preferences));
  }
  async function canLeave() {
    if (!dirty.value) return true;
    return interaction.confirm({
      title: "Выйти без сохранения?",
      cancel: "Остаться",
      text: "В форме есть несохранённые изменения. При выходе они будут отменены.",
      accept: "Отменить изменения",
      danger: true,
    });
  }
  onScopeDispose(protectUnload(() => dirty.value));
  return { draft, dirty, reset, save, canLeave };
}
