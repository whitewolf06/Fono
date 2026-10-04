<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type {
  PendingDictation,
  PendingDictationRequest,
  OverlayProcessingChoice,
} from "../domain/processing";
import { effectiveProcessingLanguage } from "../domain/processing";
import ProcessingChoiceControls from "./ProcessingChoiceControls.vue";
import AppIcon from "./AppIcon.vue";
import DictationNotice from "./DictationNotice.vue";
import { dictationNotice } from "../domain/dictationNotice";
const props = defineProps<{
  pending: PendingDictation;
  choice?: OverlayProcessingChoice;
  rememberChoice?: boolean;
  saving?: boolean;
}>();
const emit = defineEmits<{
  resolve: [request: PendingDictationRequest];
  copy: [text: string, sessionId: number];
  choice: [choice: OverlayProcessingChoice];
}>();
const selected = ref<OverlayProcessingChoice>({
  preset: props.pending.preset,
  targetLanguage: props.pending.targetLanguage,
  processingEnabled: props.pending.processingEnabled,
  translationEnabled: props.pending.translationEnabled,
});
watch(
  () => props.pending.sessionId,
  () => {
    selected.value = {
      preset: props.pending.preset,
      targetLanguage: props.pending.targetLanguage,
      processingEnabled: props.pending.processingEnabled,
      translationEnabled: props.pending.translationEnabled,
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
    preset: selected.value.preset,
    targetLanguage: effectiveProcessingLanguage(selected.value),
  });
}
const effectiveLanguage = computed(() =>
  effectiveProcessingLanguage(selected.value),
);
const notice = computed(() =>
  dictationNotice(props.pending.error, {
    insertionBlocked: props.pending.insertionBlocked,
    hasText: !!(props.pending.resultText ?? props.pending.originalText),
  }),
);
</script>
<template>
  <div class="pending-actions" data-overlay-interactive>
    <DictationNotice v-if="notice" :notice="notice" />
    <p v-else class="pending-title" role="status">
      {{
        pending.phase === "processing"
          ? "Обрабатываю текст"
          : pending.copyOnly
            ? "Текст готов"
            : "Выберите действие с текстом"
      }}
    </p>
    <p class="pending-text" :title="pending.resultText ?? pending.originalText">
      {{ pending.resultText ?? pending.originalText }}
    </p>
    <ProcessingChoiceControls
      v-if="
        pending.processingEnabled &&
        (!pending.insertionBlocked || pending.copyOnly)
      "
      :choice="selected"
      :disabled="pending.phase === 'processing'"
      :saving="saving"
      :remember="rememberChoice"
      @change="choose"
    />
    <div class="pending-footer" role="group" aria-label="Действия с диктовкой">
      <button
        v-if="!pending.copyOnly && !pending.insertionBlocked"
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
        v-if="
          pending.processingEnabled &&
          (!pending.insertionBlocked || pending.copyOnly)
        "
        type="button"
        class="dictation-action"
        :class="
          effectiveLanguage
            ? 'dictation-action--translate'
            : 'dictation-action--process'
        "
        :disabled="pending.phase === 'processing' || saving"
        @click="
          resolve(pending.copyOnly ? 'process_preview' : 'process_and_insert')
        "
      >
        {{
          pending.phase === "processing"
            ? "Обработка…"
            : effectiveLanguage
              ? "Обработать · " + effectiveLanguage.toUpperCase()
              : "Обработать"
        }}
      </button>
      <button
        type="button"
        class="dictation-action"
        :title="
          pending.copyOnly
            ? 'Копировать текст и закрыть индикатор'
            : 'Копировать текст'
        "
        aria-label="Копировать текст"
        :disabled="pending.phase === 'processing' || saving"
        @click="
          emit(
            'copy',
            pending.resultText ?? pending.originalText,
            pending.sessionId,
          )
        "
      >
        <AppIcon name="copy" :size="14" />
        <span v-if="pending.copyOnly || pending.insertionBlocked"
          >Копировать</span
        >
      </button>
      <button
        type="button"
        class="dictation-action overlay-cancel"
        :title="
          pending.copyOnly || pending.insertionBlocked
            ? 'Закрыть без копирования'
            : 'Отменить диктовку'
        "
        @click="resolve('cancel')"
      >
        <AppIcon name="x" :size="14" />{{
          pending.copyOnly || pending.insertionBlocked ? "Закрыть" : "Отмена"
        }}
      </button>
    </div>
  </div>
</template>
