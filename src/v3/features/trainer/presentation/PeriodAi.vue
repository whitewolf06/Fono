<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { WlBadge, WlButton, WlCard, WlCheckbox } from "@whitelife-core/ui-kit";
import type { TrainerReport } from "../domain/report";
import {
  periodAiDisabledReason,
  periodConsentKey,
} from "../application/periodReportController";
import { reportTime as time } from "../application/reportFormat";
import AiContextSummary from "./AiContextSummary.vue";
const props = defineProps<{
  report: TrainerReport;
  busy: boolean;
  analyzing: boolean;
  cancelling: boolean;
  connectionKey?: string;
}>();
const emit = defineEmits<{ analyze: [shareText: boolean]; cancel: [] }>();
const shareText = ref(false);
const configuration = computed(() => props.report.aiConfiguration);
const aiContext = computed(() =>
  shareText.value
    ? props.report.aiContext?.configuredShared
    : props.report.aiContext?.metricsOnly,
);
const blocked = computed(() =>
  periodAiDisabledReason(props.report, shareText.value),
);
const scopeLabels = {
  metrics_only: "числовые метрики",
  findings: "метрики и примеры речевых маркеров",
  original_text: "метрики и исходные расшифровки",
};
watch(
  () => periodConsentKey(props.report, props.connectionKey),
  () => {
    shareText.value = false;
  },
);
</script>

<template>
  <section class="period-ai" aria-label="ИИ-разбор периода">
    <WlCard class="ai-controls trainer-card">
      <div class="section-header ai-heading">
        <div>
          <h3>Выбранная LLM-модель для анализа</h3>
          <p class="selected-model">
            {{ configuration.model || "Не выбрана" }}
          </p>
        </div>
        <RouterLink
          class="text-link"
          to="/settings/processing?field=trainerModel"
          >Настройки ИИ</RouterLink
        >
      </div>
      <p class="report-connection">
        <span class="muted">Подключение</span>
        <strong>{{ configuration.profileName || "не выбрано" }}</strong>
      </p>
      <div
        v-if="configuration.dataScope !== 'metrics_only'"
        class="report-sharing"
      >
        <WlCheckbox v-model="shareText" :disabled="busy">{{
          configuration.dataScope === "original_text"
            ? "Также передать исходные расшифровки учтённых диктовок"
            : "Также передать примеры найденных речевых маркеров"
        }}</WlCheckbox>
        <p class="muted report-note">
          {{
            configuration.dataScope === "original_text"
              ? "В запрос могут попасть полные личные тексты за этот период. Без этого выбора отправляются только числовые метрики."
              : "В запрос попадут фрагменты исходных расшифровок с найденными маркерами. Без этого выбора отправляются только числовые метрики."
          }}
        </p>
      </div>
      <p v-if="configuration.cloud" class="notice report-cloud">
        Выбрано облачное подключение. При запуске
        {{
          shareText ? "числовые метрики и выбранные тексты" : "числовые метрики"
        }}
        будут переданы внешнему провайдеру.<template
          v-if="!configuration.cloudConsent"
        >
          Сначала разрешите передачу данных в настройках ИИ.</template
        >
      </p>
      <AiContextSummary v-if="aiContext" :context="aiContext" />
      <div class="actions report-actions">
        <WlButton
          size="sm"
          variant="primary"
          :loading="analyzing"
          :disabled="busy || !!blocked"
          @click="emit('analyze', shareText)"
          >Проанализировать период</WlButton
        ><WlButton
          v-if="analyzing || cancelling"
          size="sm"
          variant="soft"
          :loading="cancelling"
          :disabled="cancelling"
          @click="emit('cancel')"
          >Отменить разбор</WlButton
        >
        <p class="muted report-launch-note">
          По умолчанию передаются только числовые метрики периода.
        </p>
      </div>
      <p v-if="blocked" class="muted report-note" role="status">
        {{ blocked }}
      </p>
    </WlCard>
    <WlCard
      v-if="report.aiSummary"
      class="period-ai-result trainer-card"
      role="region"
      aria-label="Сохранённый ИИ-разбор"
    >
      <p v-if="report.aiStale" class="notice">
        Этот ИИ-разбор относится к прежним данным или настройкам.
      </p>
      <div class="saved-ai-heading">
        <h4>Сохранённый разбор</h4>
        <small class="muted"
          >{{ report.aiSummary.profileName }} · {{ report.aiSummary.model }} ·
          {{ time(report.aiSummary.generatedAt) }} UTC</small
        >
      </div>
      <p class="summary-text">{{ report.aiSummary.summary }}</p>
      <p class="muted report-note">
        Данные этого разбора: {{ scopeLabels[report.aiSummary.dataScope] }}.
      </p>
      <AiContextSummary
        v-if="report.aiSummary.context"
        :context="report.aiSummary.context"
        saved
      />
      <ol class="period-priorities">
        <li
          v-for="(priority, index) in report.aiSummary.priorities"
          :key="index"
        >
          <div class="priority-heading">
            <WlBadge :value="index + 1" variant="accent" /><strong>{{
              priority.title
            }}</strong>
          </div>
          <dl class="priority-details">
            <div>
              <dt>Основание</dt>
              <dd>{{ priority.basis }}</dd>
            </div>
            <div>
              <dt>Практика</dt>
              <dd>{{ priority.exercise }}</dd>
            </div>
          </dl>
        </li>
      </ol>
    </WlCard>
  </section>
</template>

<style scoped>
.trainer-card {
  min-width: 0;
  background: var(--fono-surface);
}
.trainer-card :deep(.wl-card__content) {
  padding: var(--fono-space-4);
}
h3,
h4 {
  margin: 0;
  font-size: var(--fono-type-lg);
}
.report-note {
  font-size: var(--fono-type-md);
  line-height: 1.7;
}
.priority-details {
  margin: 0;
}
.period-ai {
  display: grid;
  gap: var(--fono-space-5);
}
.period-ai-result :deep(.wl-card__content) {
  display: grid;
  gap: var(--fono-space-4);
}
.ai-controls :deep(.wl-card__content) {
  display: grid;
  gap: var(--fono-space-3);
}
.ai-heading {
  margin-bottom: 0;
  flex-wrap: wrap;
  gap: var(--fono-space-3);
  align-items: flex-start;
}
.ai-heading > div {
  display: grid;
  gap: var(--fono-space-1);
}
.ai-heading h3 {
  font-size: var(--fono-type-md);
  font-weight: 400;
  color: var(--fono-muted);
}
.ai-heading .selected-model {
  font-size: var(--fono-type-xl);
  font-weight: 600;
  color: var(--fono-text);
  overflow-wrap: anywhere;
}
.report-connection {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: var(--fono-space-2);
  font-size: var(--fono-type-sm);
}
.report-connection strong {
  font-weight: 500;
  overflow-wrap: anywhere;
}
.report-sharing {
  display: grid;
  gap: var(--fono-space-3);
  padding: var(--fono-space-3);
  border: 1px solid var(--fono-border);
  border-radius: var(--fono-radius);
}
.report-actions {
  flex-wrap: wrap;
  gap: var(--fono-space-3);
}
.report-launch-note {
  flex: 1 1 var(--fono-trainer-day-select-width);
  font-size: var(--fono-type-sm);
  line-height: 1.6;
}
.saved-ai-heading {
  display: grid;
  gap: var(--fono-space-2);
}
.saved-ai-heading small {
  font-size: var(--fono-type-sm);
  line-height: 1.6;
  overflow-wrap: anywhere;
}
.summary-text,
.priority-details dd {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  font-size: var(--fono-type-md);
  line-height: 1.7;
}
.period-priorities {
  display: grid;
  gap: var(--fono-space-4);
  padding: 0;
  margin: 0;
  list-style: none;
}
.period-priorities li {
  display: grid;
  gap: var(--fono-space-4);
  border-top: 1px solid var(--fono-border);
  padding-top: var(--fono-space-4);
}
.priority-heading {
  display: flex;
  align-items: center;
  gap: var(--fono-space-3);
  font-size: var(--fono-type-lg);
}
.priority-details {
  display: grid;
  gap: var(--fono-space-3);
}
.priority-details > div {
  display: grid;
  gap: var(--fono-space-1);
}
.priority-details dt {
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
}
.priority-details dd {
  margin: 0;
  color: var(--fono-text);
}
</style>
