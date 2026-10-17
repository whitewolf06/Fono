<script setup lang="ts">
import { computed, ref } from "vue";
import {
  WlAccordion,
  WlBadge,
  WlButton,
  WlStatCard,
} from "@whitelife-core/ui-kit";
import type { TrainerReport } from "../domain/report";
import { formatMetric } from "../application/overview";
import {
  reportDate as date,
  reportTime as time,
} from "../application/reportFormat";
const props = defineProps<{
  report: TrainerReport;
  loading: boolean;
  busy: boolean;
  error: string;
  native: boolean;
}>();
const emit = defineEmits<{ refresh: [] }>();
const snapshot = computed(() => props.report.snapshot);
const sourceOpenKeys = ref<string[]>([]);
const sourceItems = [{ key: "trainer-sources", title: "Состав выборки" }];
const markerCount = computed(() =>
  Object.values(snapshot.value.counts).reduce((sum, count) => sum + count, 0),
);
const sourceCount = computed(
  () =>
    snapshot.value.includedSessions +
    snapshot.value.excludedSessions +
    snapshot.value.pendingSessions +
    snapshot.value.unavailableSessions,
);
const lastDate = computed(() =>
  date(new Date(Date.parse(snapshot.value.to) - 1).toISOString()),
);
</script>

<template>
  <div class="report-heading">
    <div class="report-context">
      <div class="report-range">
        <h2>{{ date(snapshot.from) }} — {{ lastDate }}</h2>
        <span class="muted">UTC</span>
        <WlBadge
          :value="
            loading
              ? 'Проверяем данные'
              : report.stale
                ? 'Есть новые данные'
                : error
                  ? 'Есть ошибка'
                  : 'Актуален'
          "
          :variant="report.stale || error ? 'warn' : 'success'"
        />
      </div>
      <div class="report-meta">
        <span>Снимок: {{ time(snapshot.generatedAt) }} UTC</span>
        <WlBadge v-if="!native" value="Демонстрация" variant="gray" />
      </div>
    </div>
    <WlButton
      size="sm"
      variant="primary"
      :loading="loading"
      :disabled="busy || (!report.stale && !error)"
      @click="emit('refresh')"
    >
      {{ error && !report.stale ? "Повторить проверку" : "Обновить отчёт" }}
    </WlButton>
  </div>
  <p v-if="report.stale" class="notice report-stale" role="status">
    Данные изменились. Обновите отчёт, чтобы учесть новые записи. Числа ниже
    относятся к сохранённому снимку.
  </p>
  <p v-if="error" class="error-text" role="alert">{{ error }}</p>
  <div class="report-metrics" aria-label="Сводка периода">
    <WlStatCard
      label="Диктовок"
      :value="`${snapshot.includedSessions} из ${sourceCount}`"
    />
    <WlStatCard label="Слов" :value="formatMetric(snapshot.totalWords)" />
    <WlStatCard label="Маркеров" :value="String(markerCount)" />
    <WlStatCard label="На 100 слов" :value="formatMetric(snapshot.density)" />
  </div>
  <WlAccordion
    v-model:open-keys="sourceOpenKeys"
    :items="sourceItems"
    class="report-sources"
  >
    <template #item>
      <div class="source-details">
        <dl class="source-counts">
          <div>
            <dt>Исключено</dt>
            <dd>{{ snapshot.excludedSessions }}</dd>
          </div>
          <div>
            <dt>В очереди</dt>
            <dd>{{ snapshot.pendingSessions }}</dd>
          </div>
          <div>
            <dt>Без разбора</dt>
            <dd>{{ snapshot.unavailableSessions }}</dd>
          </div>
        </dl>
        <p>
          В отчёт входят записи с сохранённой исходной расшифровкой и готовым
          локальным разбором. Включение записи в аналитику не восстанавливает
          исходник, если он не сохранялся.
        </p>
        <p class="muted">
          <template v-if="snapshot.historyLimit != null"
            >Проверяются последние
            {{ snapshot.historyLimit }} записей.</template
          >
          <template v-else
            >Проверяются все сохранённые записи за выбранный период.</template
          >
          Исходные тексты хранятся до
          {{ snapshot.retentionDays }} дней.<template
            v-if="snapshot.coverageStart"
          >
            Доступная история — с
            {{ date(snapshot.coverageStart) }}.</template
          >
          Выбранный период может быть длиннее доступной истории.
        </p>
        <div class="source-links">
          <RouterLink class="text-link" to="/history"
            >Открыть историю</RouterLink
          ><RouterLink class="text-link" to="/settings/privacy"
            >Хранение данных</RouterLink
          ><small class="muted">Правила: {{ snapshot.rulesVersion }}</small>
        </div>
      </div>
    </template>
  </WlAccordion>
  <p v-if="!snapshot.includedSessions" class="notice report-sample">
    Пока нет готовых диктовок за этот период. Состав выборки выше объясняет,
    какие записи не вошли.
  </p>
  <p v-else-if="snapshot.insufficientData" class="notice report-sample">
    Мало данных для общего вывода. ИИ-разбор доступен от 3 диктовок и 100 слов.
  </p>
</template>

<style scoped>
.report-heading,
.report-range,
.report-meta,
.source-links {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--fono-space-2) var(--fono-space-3);
}
.report-heading {
  justify-content: space-between;
  align-items: flex-start;
  gap: var(--fono-space-4);
  margin-bottom: var(--fono-space-4);
}
.report-context {
  display: grid;
  gap: var(--fono-space-2);
}
.report-range h2 {
  margin: 0;
  font-size: var(--fono-type-xl);
}
.report-range > span {
  font-size: var(--fono-type-sm);
}
.report-meta {
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
}
.report-stale,
.report-sample {
  margin: 0 0 var(--fono-space-4);
  font-size: var(--fono-type-md);
}
.error-text {
  margin-bottom: var(--fono-space-4);
}
.report-metrics {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: var(--fono-space-3);
}
.report-metrics :deep(.wl-card) {
  min-width: 0;
  background: var(--fono-surface);
}
.report-metrics :deep(.wl-card__content) {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--fono-space-2);
  padding: var(--fono-space-2) var(--fono-space-3);
}
.report-metrics :deep(.wl-stat-card__label) {
  text-transform: none;
  letter-spacing: normal;
  font-size: var(--fono-type-sm);
}
.report-metrics :deep(.wl-stat-card__value) {
  margin-top: 0;
  font-size: var(--fono-type-xl);
  line-height: 1.5;
  white-space: nowrap;
}
.report-sources {
  max-width: none;
  margin-block: var(--fono-space-4);
}
.source-details {
  display: grid;
  gap: var(--fono-space-4);
  font-size: var(--fono-type-md);
  line-height: 1.7;
}
.source-counts,
.priority-details {
  margin: 0;
}
.source-counts {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: var(--fono-space-3);
}
.source-counts > div {
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  align-items: center;
  gap: var(--fono-space-2);
  padding: var(--fono-space-2) var(--fono-space-3);
  background: var(--fono-surface-raised);
  border-radius: var(--fono-radius-sm);
}
.source-counts dt {
  color: var(--fono-muted);
}
.source-counts dd {
  margin: 0;
  color: var(--fono-text);
  font-weight: 600;
}
.source-links {
  gap: var(--fono-space-3) var(--fono-space-5);
}
.source-links small {
  font-size: var(--fono-type-sm);
}

@media (max-width: 780px) {
  .report-metrics {
    gap: var(--fono-space-2);
  }
  .report-metrics :deep(.wl-card__content) {
    display: grid;
    align-content: start;
    justify-content: start;
    gap: var(--fono-space-1);
    padding: var(--fono-space-2) var(--fono-space-3);
  }
  .report-metrics :deep(.wl-stat-card__label) {
    font-size: var(--fono-type-sm);
  }
}
</style>
