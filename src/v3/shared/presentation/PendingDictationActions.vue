<script setup lang="ts">
import { ref, watch } from "vue";
import type {
  PendingDictation,
  PendingDictationRequest,
  ProcessingPreset,
  ProcessingTranslation,
} from "../domain/processing";
import { presetOptions, translationOptions } from "../domain/processing";
import AppIcon from "./AppIcon.vue";
const props = defineProps<{ pending: PendingDictation }>();
const emit = defineEmits<{
  resolve: [request: PendingDictationRequest];
  copy: [text: string];
}>();
const preset = ref<ProcessingPreset>(props.pending.preset);
const translation = ref<ProcessingTranslation>(
  props.pending.targetLanguage ?? "none",
);
watch(
  () => props.pending.sessionId,
  () => {
    preset.value = props.pending.preset;
    translation.value = props.pending.targetLanguage ?? "none";
  },
);
function resolve(action: PendingDictationRequest["action"]) {
  emit("resolve", {
    sessionId: props.pending.sessionId,
    action,
    preset: preset.value,
    targetLanguage: translation.value === "none" ? null : translation.value,
  });
}
</script>
<template>
  <div class="pending-actions" data-overlay-interactive>
    <p class="pending-text" :title="pending.resultText ?? pending.originalText">
      {{ pending.resultText ?? pending.originalText }}
    </p>
    <template v-if="pending.processingEnabled && !pending.insertionBlocked">
      <div class="pending-presets" role="group" aria-label="Режим обработки">
        <button
          v-for="option in presetOptions"
          :key="option.value"
          class="dictation-action dictation-action--process"
          :aria-pressed="preset === option.value"
          :disabled="pending.phase === 'processing'"
          @click="preset = option.value"
        >
          {{ option.label }}
        </button>
      </div>
      <div
        class="pending-languages"
        role="group"
        aria-label="Перевод после обработки"
      >
        <span>Перевод</span>
        <button
          v-for="option in translationOptions"
          :key="option.value"
          class="dictation-action dictation-action--translate"
          :title="option.label"
          :aria-label="option.label"
          :aria-pressed="translation === option.value"
          :disabled="pending.phase === 'processing'"
          @click="translation = option.value"
        >
          {{ option.value === "none" ? "Нет" : option.value.toUpperCase() }}
        </button>
      </div>
    </template>
    <p v-if="pending.error" class="pending-error" role="alert">
      {{ pending.error }}
    </p>
    <div class="pending-footer" role="group" aria-label="Действия с диктовкой">
      <button
        class="dictation-action dictation-action--insert"
        :title="
          pending.source === 'ui'
            ? 'Оставить исходный текст в Fono'
            : 'Вставить исходный текст без обработки'
        "
        :disabled="pending.phase === 'processing' || pending.insertionBlocked"
        @click="resolve('insert_raw')"
      >
        Исходный
      </button>
      <button
        v-if="pending.processingEnabled && !pending.insertionBlocked"
        class="dictation-action"
        :class="
          translation === 'none'
            ? 'dictation-action--process'
            : 'dictation-action--translate'
        "
        :disabled="pending.phase === 'processing'"
        @click="resolve('process_and_insert')"
      >
        {{
          pending.phase === "processing"
            ? "Обработка…"
            : translation === "none"
              ? "Обработать"
              : "Обработать · " + translation.toUpperCase()
        }}
      </button>
      <button
        class="dictation-action"
        title="Копировать текст"
        aria-label="Копировать текст"
        @click="emit('copy', pending.resultText ?? pending.originalText)"
      >
        <AppIcon name="copy" :size="14" />
      </button>
      <button
        class="dictation-action dictation-action--cancel"
        title="Отменить диктовку"
        aria-label="Отменить диктовку"
        @click="resolve('cancel')"
      >
        <AppIcon name="x" :size="14" />
      </button>
    </div>
  </div>
</template>
