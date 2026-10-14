<script setup lang="ts">
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
      <h3 id="feedback-title">Обратная связь</h3>
      <WlButton
        size="sm"
        :disabled="!channel.available || busy"
        :loading="busy"
        @click="run(() => channel.open(includeContext))"
      >
        <template #icon><AppIcon name="external-link" /></template>
        Открыть форму в браузере
      </WlButton>
    </div>
    <p class="muted">
      Отзыв, идея или ошибка — в отдельной форме. Сообщение отправляется только
      после подтверждения в браузере.
    </p>
    <p v-if="!channel.available" class="notice" role="status">
      Форма обратной связи пока не настроена для этой сборки.
    </p>
    <div class="feedback-context-option">
      <span id="feedback-context-label">Добавить версию и раздел Fono</span>
      <WlSwitch
        v-model="includeContext"
        :disabled="!channel.available || busy"
        aria-labelledby="feedback-context-label"
      />
    </div>
    <p v-if="includeContext" class="muted" aria-live="polite">
      {{ channel.context.version }} · {{ channel.context.platform }} ·
      {{ channel.context.route }}. В форме можно проверить и убрать эти поля.
    </p>
    <p class="muted">
      Тексты диктовок, журнал, аудио, ключи и имена устройств не добавляются.
    </p>
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
