<script setup lang="ts">
import { WlTooltip as vTooltip } from "@whitelife-core/ui-kit";
import { message } from "../domain/messages";
import AppIcon from "./AppIcon.vue";
defineProps<{ detail: string }>();
</script>
<template>
  <span
    v-tooltip="detail"
    class="error-hint"
    tabindex="0"
    role="alert"
    :aria-label="`${message('health.error')}: ${detail}`"
  >
    <AppIcon name="warn" :size="16" />
    {{ message("health.error") }}
  </span>
</template>
<style scoped>
.error-hint {
  display: inline-flex;
  align-items: center;
  gap: var(--fono-space-1);
  color: var(--fono-error);
  font-size: var(--fono-type-sm);
  cursor: help;
}
</style>
