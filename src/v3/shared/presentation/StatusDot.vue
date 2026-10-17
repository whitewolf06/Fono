<script setup lang="ts">
import { message } from "../domain/messages";
import type { Health } from "../domain/contracts";
defineProps<{ health: Health; label?: string }>();
const labels: Record<Health, string> = {
  ready: message("health.ready"),
  off: message("health.off"),
  missing: message("health.missing"),
  loading: message("health.loading"),
  error: message("health.error"),
};
</script>
<template>
  <span class="status" :data-health="health"
    ><span class="status-dot" aria-hidden="true" />{{
      label || labels[health]
    }}</span
  >
</template>
