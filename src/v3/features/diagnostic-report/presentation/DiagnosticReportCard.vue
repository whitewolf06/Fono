<script setup lang="ts">
import { ref } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import type { DiagnosticReportPort } from "../../../shared/domain/diagnosticReport";
import { useFeedback } from "../../../shared/application/feedback";
import { useDiagnosticReport } from "../application/useDiagnosticReport";
import AppIcon from "../../../shared/presentation/AppIcon.vue";

const props = defineProps<{
  port: DiagnosticReportPort;
  copy: (text: string) => Promise<void>;
}>();
const { preview, createAndCopy } = useDiagnosticReport(props.port, props.copy);
const { run, busy, error } = useFeedback();
const expanded = ref(false);
async function generate() {
  const copied = await run(createAndCopy, "Отчёт создан и скопирован");
  if (!copied && preview.value) expanded.value = true;
}
</script>

<template>
  <section
    class="diagnostic-report-card"
    aria-labelledby="diagnostic-report-title"
  >
    <div class="section-header">
      <h3 id="diagnostic-report-title">Отчёт диагностики</h3>
      <WlButton size="sm" :loading="busy" :disabled="busy" @click="generate">
        <template #icon><AppIcon name="copy" /></template>
        Создать и скопировать
      </WlButton>
    </div>
    <p class="muted">
      Сборка, состояния компонентов и задержки последней диктовки. Без текстов,
      аудио, словаря, ключей, имён устройств и журнала. Никуда не отправляется.
    </p>
    <p v-if="error" class="diagnostic-report-error" role="alert">{{ error }}</p>
    <template v-if="preview">
      <WlButton
        size="sm"
        variant="ghost"
        :aria-expanded="expanded"
        aria-controls="diagnostic-report-preview"
        @click="expanded = !expanded"
        >{{ expanded ? "Скрыть отчёт" : "Посмотреть отчёт" }}</WlButton
      >
      <pre
        v-if="expanded"
        id="diagnostic-report-preview"
        class="diagnostic-report-preview"
        tabindex="0"
        aria-label="Текст отчёта диагностики"
        >{{ preview }}</pre>
    </template>
  </section>
</template>

<style scoped>
.diagnostic-report-card {
  display: grid;
  gap: var(--fono-space-3);
  padding: var(--fono-space-4);
  background: var(--fono-surface);
  border: 1px solid var(--fono-border);
  border-radius: var(--fono-radius);
}
.diagnostic-report-card .section-header {
  flex-wrap: wrap;
  gap: var(--fono-space-3);
}
.diagnostic-report-card p {
  margin: 0;
}
.diagnostic-report-error {
  color: var(--fono-error);
}
.diagnostic-report-preview {
  min-width: 0;
  margin: 0;
  padding: var(--fono-space-3);
  background: var(--fono-canvas);
  border-radius: var(--fono-radius-sm);
  color: var(--fono-secondary);
  font-size: var(--fono-type-sm);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  user-select: text;
}
</style>
