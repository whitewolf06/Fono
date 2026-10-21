<script setup lang="ts">
import { message } from "../domain/messages";
import type { Health } from "../domain/contracts";
defineProps<{ health: Health; label?: string }>();
const labels: Record<Health, string> = {
  get ready() {
    return message("health.ready");
  },
  get off() {
    return message("health.off");
  },
  get missing() {
    return message("health.missing");
  },
  get loading() {
    return message("health.loading");
  },
  get error() {
    return message("health.error");
  },
};
</script>
<template>
  <span class="status" :data-health="health"
    ><span class="status-dot" aria-hidden="true" />{{
      label || labels[health]
    }}</span
  >
</template>
