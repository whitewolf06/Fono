<script setup lang="ts">
import { message as $t } from "../domain/messages";
import AppIcon from "./AppIcon.vue";
defineProps<{
  id: string;
  title?: string;
  hint?: string;
  initiallyOpen?: boolean;
}>();
</script>
<template>
  <details :id="id" class="settings-disclosure" :open="initiallyOpen">
    <summary>
      <span>{{ title || $t("settings.advanced") }}</span>
      <AppIcon name="chevron-down" :size="16" />
    </summary>
    <div class="form-stack settings-disclosure-body">
      <p v-if="hint" class="muted">{{ hint }}</p>
      <slot />
    </div>
  </details>
</template>
