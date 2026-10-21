<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

import { ref } from "vue";
import { WlButton, WlSwitch } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
const channel = useWorkspace().feedback;
const includeContext = ref(false);
const { run, busy, error } = useFeedback();
</script>
<template>
  <section id="feedback" class="feedback-card" aria-labelledby="feedback-title">
    <div class="section-header">
      <h3 id="feedback-title">{{ $t("ui.930f9a65fb") }}</h3>
      <WlButton
        size="sm"
        :disabled="!channel.available || busy"
        :loading="busy"
        @click="run(() => channel.open(includeContext))"
      >
        <template #icon><AppIcon name="external-link" /></template>
        {{ $t("ui.2f7192e959") }}
      </WlButton>
    </div>
    <p class="muted">{{ $t("ui.60ad7a5c32") }}</p>
    <p v-if="!channel.available" class="notice" role="status">
      {{ $t("ui.df9e9f065b") }}
    </p>
    <div class="feedback-context-option">
      <span id="feedback-context-label">{{ $t("ui.318881e364") }}</span>
      <WlSwitch
        v-model="includeContext"
        :disabled="!channel.available || busy"
        aria-labelledby="feedback-context-label"
      />
    </div>
    <p v-if="includeContext" class="muted" aria-live="polite">
      {{ channel.context.version }} · {{ channel.context.platform }} ·
      {{ channel.context.route }}{{ $t("ui.3a19b573c7") }}
    </p>
    <p class="muted">{{ $t("ui.d88e003074") }}</p>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
  </section>
</template>
<style scoped>
.feedback-card {
  min-width: 0;
  display: grid;
  gap: var(--fono-space-3);
  padding: var(--fono-space-4);
  background: var(--fono-surface);
  border: 1px solid var(--fono-border);
  border-radius: var(--fono-radius);
}
.feedback-card .section-header {
  flex-wrap: wrap;
  gap: var(--fono-space-3);
}
.feedback-card p {
  margin: 0;
}
.feedback-context-option {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--fono-space-3);
}
.feedback-context-option :deep(.wl-switch-wrap) {
  flex-shrink: 0;
}
</style>
