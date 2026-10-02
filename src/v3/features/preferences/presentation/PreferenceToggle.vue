<script setup lang="ts">
import { WlSwitch } from "@whitelife-core/ui-kit";
import type { ToggleKey } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
import { useFeedback } from "../../../shared/application/feedback";
const props = defineProps<{
  name: ToggleKey;
  label: string;
  description?: string;
  compact?: boolean;
}>();
const workspace = useWorkspace();
const ui = useInteraction();
const { run } = useFeedback();
async function change(value: boolean) {
  if (
    props.name === "trainerEnabled" &&
    value &&
    !workspace.state.preferences.analyticsConsent
  ) {
    if (
      !(await ui.confirm({
        title: "Включить речевого тренера?",
        text:
          "Для анализа Fono будет сохранять исходные расшифровки локально. Их можно удалить в любой момент. Отправка в ИИ включается отдельно." +
          (workspace.native
            ? ""
            : " Сейчас используются только демонстрационные данные."),
        accept: "Разрешить и включить",
      }))
    )
      return;
    workspace.state.pending.trainerEnabled = true;
    await run(() =>
      workspace.settings.save({ analyticsConsent: true, trainerEnabled: true }),
    );
    workspace.state.pending.trainerEnabled = false;
  } else if (props.name === "cloudConsent" && !value) {
    await run(() =>
      workspace.settings.save({ cloudConsent: false, trainerAiEnabled: false }),
    );
  } else if (props.name === "analyticsConsent" && !value) {
    if (
      !(await ui.confirm({
        title: "Отключить аналитику?",
        text: workspace.native
          ? "Речевой тренер выключится. Исходные расшифровки и данные анализа будут удалены; итоговые тексты останутся."
          : "Речевой тренер также выключится. Уже накопленные данные можно отдельно удалить ниже.",
        accept: "Отключить",
      }))
    )
      return;
    await run(() =>
      workspace.settings.save({
        analyticsConsent: false,
        trainerEnabled: false,
        trainerAiEnabled: false,
      }),
    );
  } else await run(() => workspace.settings.toggle(props.name, value));
}
</script>
<template>
  <div
    class="toggle-row"
    :class="{ 'is-compact': compact }"
    :aria-busy="workspace.state.pending[name] || undefined"
  >
    <div v-if="!compact">
      <strong>{{ label }}</strong>
      <p v-if="description">{{ description }}</p>
    </div>
    <WlSwitch
      :model-value="workspace.state.preferences[name]"
      :disabled="workspace.state.pending[name]"
      :aria-label="label"
      @update:model-value="change"
    />
    <span v-if="workspace.state.pending[name]" class="sr-only" role="status"
      >Сохранение</span
    >
  </div>
</template>
