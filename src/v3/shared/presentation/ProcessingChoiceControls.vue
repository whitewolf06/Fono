<script setup lang="ts">
import { computed } from "vue";
import type { OverlayProcessingChoice } from "../domain/processing";
import {
  presetOptions,
  translationOptions,
  effectiveProcessingLanguage,
} from "../domain/processing";
const props = defineProps<{
  choice: OverlayProcessingChoice;
  disabled?: boolean;
  saving?: boolean;
  remember?: boolean;
}>();
defineEmits<{ change: [choice: OverlayProcessingChoice] }>();
const effectiveLanguage = computed(() =>
  effectiveProcessingLanguage(props.choice),
);
</script>
<template>
  <div class="processing-choice" data-overlay-interactive>
    <div
      class="processing-choice-row"
      role="group"
      aria-label="Стиль обработки"
    >
      <span class="processing-choice-label">Стиль</span>
      <div class="processing-choice-options">
        <button
          type="button"
          v-for="option in presetOptions"
          :key="option.value"
          class="dictation-action dictation-action--process"
          :aria-pressed="choice.preset === option.value"
          :disabled="disabled || saving"
          :title="option.label"
          @click="$emit('change', { ...choice, preset: option.value })"
        >
          {{ option.value === "formal" ? "Деловой" : option.label }}
        </button>
      </div>
    </div>
    <div
      class="processing-choice-row"
      role="group"
      aria-label="Перевод после обработки"
    >
      <span class="processing-choice-label">Перевод</span>
      <div class="processing-choice-options">
        <button
          type="button"
          v-for="option in translationOptions"
          :key="option.value"
          class="dictation-action dictation-action--translate"
          :aria-pressed="(effectiveLanguage ?? 'none') === option.value"
          :aria-label="option.label"
          :disabled="disabled || saving"
          :title="option.label"
          @click="
            $emit('change', {
              ...choice,
              targetLanguage:
                option.value === 'none' ? choice.targetLanguage : option.value,
              translationEnabled: option.value !== 'none',
            })
          "
        >
          {{ option.value === "none" ? "Нет" : option.value.toUpperCase() }}
        </button>
      </div>
    </div>
    <small v-if="remember" class="processing-choice-note" role="status">
      {{
        saving ? "Сохраняю выбор…" : "Выбор сохраняется для следующих диктовок"
      }}
    </small>
  </div>
</template>
