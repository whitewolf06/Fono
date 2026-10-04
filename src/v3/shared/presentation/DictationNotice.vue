<script setup lang="ts">
import { watch, ref } from "vue";
import type { DictationNotice } from "../domain/dictationNotice";
import AppIcon from "./AppIcon.vue";
const props = withDefaults(
  defineProps<{ notice: DictationNotice; showTitle?: boolean }>(),
  { showTitle: true },
);
const expanded = ref(false);
watch(
  () => props.notice.details,
  () => {
    expanded.value = false;
  },
);
</script>
<template>
  <div class="dictation-notice" data-overlay-interactive>
    <div class="dictation-notice-summary" role="alert">
      <span v-if="showTitle" class="dictation-notice-title">
        <AppIcon name="info" :size="18" />
        <strong>{{ notice.title }}</strong>
      </span>
      <p>{{ notice.hint }}</p>
    </div>
    <details
      v-if="notice.details"
      class="dictation-notice-details"
      :open="expanded"
      @toggle="expanded = ($event.target as HTMLDetailsElement).open"
    >
      <summary>Подробнее</summary>
      <p>{{ notice.details }}</p>
    </details>
  </div>
</template>
