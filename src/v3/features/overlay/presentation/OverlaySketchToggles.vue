<script setup lang="ts">
import { WlSwitch } from "@whitelife-core/ui-kit";
import type { IndicatorChoice } from "../domain/indicator";
import {
  presetOptions,
  translationOptions,
} from "../../../shared/domain/processing";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
defineProps<{
  choice: IndicatorChoice;
  disabled: boolean;
  panel: "help" | "processing" | "translation" | null;
  panelId: string;
}>();
defineEmits<{
  choose: [choice: Partial<IndicatorChoice>];
  configure: [panel: "processing" | "translation"];
}>();
</script>
<template>
  <div class="overlay-sketch-toggles">
    <div
      class="overlay-sketch-toggle"
      :class="{ active: choice.postprocessingOn }"
    >
      <span
        class="overlay-sketch-toggle-icon"
        title="Постобработка"
        aria-hidden="true"
      >
        <AppIcon name="sparkle" :size="18" />
      </span>
      <WlSwitch
        size="sm"
        :model-value="choice.postprocessingOn"
        :disabled="disabled"
        aria-label="Постобработка"
        title="Включить или выключить постобработку"
        @update:model-value="
          (value: boolean) => $emit('choose', { postprocessingOn: value })
        "
      />
      <button
        type="button"
        class="overlay-sketch-option"
        :title="`Постобработка: ${presetOptions.find((item) => item.value === choice.style)?.label}. Выбрать стиль`"
        aria-label="Выбрать стиль обработки"
        :aria-expanded="panel === 'processing'"
        :aria-controls="panelId"
        :disabled="disabled || !choice.postprocessingOn"
        @click="$emit('configure', 'processing')"
      >
        <AppIcon name="settings" :size="16" />
      </button>
    </div>
    <div
      class="overlay-sketch-toggle translation"
      :class="{ active: choice.postprocessingOn && choice.translationOn }"
    >
      <span
        class="overlay-sketch-toggle-icon"
        title="Перевод"
        aria-hidden="true"
      >
        <AppIcon name="translate" :size="18" />
      </span>
      <WlSwitch
        size="sm"
        :model-value="choice.postprocessingOn && choice.translationOn"
        :disabled="disabled || !choice.postprocessingOn"
        aria-label="Перевод"
        :title="
          choice.postprocessingOn
            ? 'Перевод после обработки'
            : 'Включите постобработку для перевода'
        "
        @update:model-value="
          (value: boolean) => $emit('choose', { translationOn: value })
        "
      />
      <button
        type="button"
        class="overlay-sketch-option"
        :title="`Перевод: ${translationOptions.find((item) => item.value === choice.language)?.label}. Выбрать язык`"
        aria-label="Выбрать язык перевода"
        :aria-expanded="panel === 'translation'"
        :aria-controls="panelId"
        :disabled="disabled || !choice.postprocessingOn"
        @click="$emit('configure', 'translation')"
      >
        <AppIcon name="settings" :size="16" />
      </button>
    </div>
  </div>
</template>
