<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

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
  panel: "help" | "processing" | "translation" | "recognition" | null;
  panelId: string;
}>();
defineEmits<{
  choose: [choice: Partial<IndicatorChoice>];
  configure: [panel: "processing" | "translation"];
}>();
</script>
<template>
  <div class="overlay-indicator-toggles">
    <div
      class="overlay-indicator-toggle"
      :class="{ active: choice.postprocessingOn }"
    >
      <span
        class="overlay-indicator-toggle-icon"
        :title="$t('ui.b48611765d')"
        aria-hidden="true"
      >
        <AppIcon name="sparkle" :size="18" />
      </span>
      <WlSwitch
        size="sm"
        :model-value="choice.postprocessingOn"
        :disabled="disabled"
        :aria-label="$t('ui.b48611765d')"
        :title="$t('ui.9971b8f10a')"
        @update:model-value="
          (value: boolean) => $emit('choose', { postprocessingOn: value })
        "
      />
      <button
        type="button"
        class="overlay-indicator-option"
        :title="
          $t('ui.2e916f082a', {
            v0: String(
              presetOptions.find((item) => item.value === choice.style)?.label,
            ),
          })
        "
        :aria-label="$t('ui.20ebcdfa6f')"
        :aria-expanded="panel === 'processing'"
        :aria-controls="panelId"
        :disabled="disabled || !choice.postprocessingOn"
        @click="$emit('configure', 'processing')"
      >
        <AppIcon name="settings" :size="16" />
      </button>
    </div>
    <div
      class="overlay-indicator-toggle translation"
      :class="{ active: choice.postprocessingOn && choice.translationOn }"
    >
      <span
        class="overlay-indicator-toggle-icon"
        :title="$t('ui.c581c83538')"
        aria-hidden="true"
      >
        <AppIcon name="translate" :size="18" />
      </span>
      <WlSwitch
        size="sm"
        :model-value="choice.postprocessingOn && choice.translationOn"
        :disabled="disabled || !choice.postprocessingOn"
        :aria-label="$t('ui.c581c83538')"
        :title="
          choice.postprocessingOn ? $t('ui.6dbccf713c') : $t('ui.6b1ab417ca')
        "
        @update:model-value="
          (value: boolean) => $emit('choose', { translationOn: value })
        "
      />
      <button
        type="button"
        class="overlay-indicator-option"
        :title="
          $t('ui.9acb843621', {
            v0: String(
              translationOptions.find((item) => item.value === choice.language)
                ?.label,
            ),
          })
        "
        :aria-label="$t('ui.b5b3e5d9c6')"
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
