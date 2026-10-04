<script setup lang="ts">
import { ref, watch } from "vue";
import type {
  PendingDictation,
  PendingDictationRequest,
  OverlayProcessingChoice,
} from "../domain/processing";
import ProcessingChoiceControls from "./ProcessingChoiceControls.vue";
import AppIcon from "./AppIcon.vue";
const props = defineProps<{
  pending: PendingDictation;
  choice?: OverlayProcessingChoice;
  rememberChoice?: boolean;
  saving?: boolean;
}>();
const emit = defineEmits<{
  resolve: [request: PendingDictationRequest];
  copy: [text: string];
  choice: [choice: OverlayProcessingChoice];
}>();
const selected = ref<OverlayProcessingChoice>({
  preset: props.pending.preset,
  targetLanguage: props.pending.targetLanguage,
});
watch(
  () => props.pending.sessionId,
  () => {
    selected.value = {
      preset: props.pending.preset,
      targetLanguage: props.pending.targetLanguage,
    };
  },
);
watch(
  () => props.choice,
  (value) => {
    if (value) selected.value = { ...value };
  },
  { immediate: true },
);
function choose(choice: OverlayProcessingChoice) {
  selected.value = { ...choice };
  if (props.rememberChoice) emit("choice", choice);
}
function resolve(action: PendingDictationRequest["action"]) {
  emit("resolve", {
    sessionId: props.pending.sessionId,
    action,
    ...selected.value,
  });
}
</script>
<template>
  <div class="pending-actions" data-overlay-interactive>
    <p class="pending-text" :title="pending.resultText ?? pending.originalText">
      {{ pending.resultText ?? pending.originalText }}
    </p>
    <ProcessingChoiceControls
      v-if="pending.processingEnabled && !pending.insertionBlocked"
      :choice="selected"
      :disabled="pending.phase === 'processing'"
      :saving="saving"
      :remember="rememberChoice"
      @change="choose"
    />
    <p v-if="pending.error" class="pending-error" role="alert">
      {{ pending.error }}
    </p>
    <div class="pending-footer" role="group" aria-label="Действия с диктовкой">
      <button
        type="button"
        class="dictation-action dictation-action--insert"
        :title="
          pending.source === 'ui'
            ? 'Оставить исходный текст в Fono'
            : 'Вставить исходный текст без обработки'
        "
        :disabled="
          pending.phase === 'processing' || pending.insertionBlocked || saving
        "
        @click="resolve('insert_raw')"
      >
        Исходный
      </button>
      <button
        v-if="pending.processingEnabled && !pending.insertionBlocked"
        type="button"
        class="dictation-action"
        :class="
          selected.targetLanguage
            ? 'dictation-action--translate'
            : 'dictation-action--process'
        "
        :disabled="pending.phase === 'processing' || saving"
        @click="resolve('process_and_insert')"
      >
        {{
          pending.phase === "processing"
            ? "Обработка…"
            : selected.targetLanguage
              ? "Обработать · " + selected.targetLanguage.toUpperCase()
              : "Обработать"
        }}
      </button>
      <button
        type="button"
        class="dictation-action"
        title="Копировать текст"
        aria-label="Копировать текст"
        @click="emit('copy', pending.resultText ?? pending.originalText)"
      >
        <AppIcon name="copy" :size="14" />
      </button>
      <button
        type="button"
        class="dictation-action overlay-cancel"
        title="Отменить диктовку"
        @click="resolve('cancel')"
      >
        <AppIcon name="x" :size="14" />Отмена
      </button>
    </div>
  </div>
</template>
