<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  WlAccordion,
  WlBadge,
  WlButton,
  WlCard,
  WlCheckbox,
  WlStatCard,
  WlTabs,
} from "@whitelife-core/ui-kit";
import type { TrainerReport, TrainerTerm } from "../domain/report";
import {
  periodAiDisabledReason,
  periodConsentKey,
} from "../application/periodReportController";
import { formatMetric } from "../application/overview";
import DailyChart from "./DailyChart.vue";
import AiContextSummary from "./AiContextSummary.vue";

const props = defineProps<{
  report: TrainerReport;
  loading: boolean;
  analyzing: boolean;
  cancelling: boolean;
  error: string;
  native: boolean;
  connectionKey?: string;
}>();
const view = defineModel<string>("view", { default: "trainer-overview" });
const emit = defineEmits<{
  refresh: [];
  analyze: [shareText: boolean];
  cancel: [];
  selectTerm: [term: TrainerTerm];
}>();
const shareText = ref(false);
const showAllTerms = ref(false);
const sourceOpenKeys = ref<string[]>([]);
const sourceItems = [{ key: "trainer-sources", title: "Состав выборки" }];
const snapshot = computed(() => props.report.snapshot);
const configuration = computed(() => props.report.aiConfiguration);
const aiContext = computed(() =>
  shareText.value
    ? props.report.aiContext?.configuredShared
    : props.report.aiContext?.metricsOnly,
);
const blocked = computed(() =>
  periodAiDisabledReason(props.report, shareText.value),
);
const busy = computed(
  () => props.loading || props.analyzing || props.cancelling,
);
const markerCount = computed(
  () =>
    snapshot.value.counts.filler +
    snapshot.value.counts.repetition +
    snapshot.value.counts.selfCorrection +
    snapshot.value.counts.unfinished,
);
const sourceCount = computed(
  () =>
    snapshot.value.includedSessions +
    snapshot.value.excludedSessions +
    snapshot.value.pendingSessions +
    snapshot.value.unavailableSessions,
);
const terms = computed(() =>
  showAllTerms.value ? snapshot.value.terms : snapshot.value.terms.slice(0, 6),
);
const tabs = computed(() => [
  { key: "trainer-overview", label: "Обзор" },
  {
    key: "trainer-dictations",
    label: "Диктовки",
    count: snapshot.value.includedSessions,
  },
  {
    key: "trainer-ai",
    label: props.analyzing ? "Разбор ИИ · в работе" : "Разбор ИИ",
  },
]);
// The pinned kit unmounts inactive panels. Keep feature panels mounted so tab changes
// preserve explicit consent, running requests and the selected dictation.
const tabsPt = {
  tabList: { "aria-label": "Разделы речевого тренера" },
  tabpanels: { root: { hidden: true, style: { display: "none" } } },
};
const kindLabels = {
  filler: "Слово-паразит",
  repetition: "Повтор",
  self_correction: "Самоисправление",
  unfinished: "Незавершённая фраза",
};
const scopeLabels = {
  metrics_only: "числовые метрики",
  findings: "метрики и примеры речевых маркеров",
  original_text: "метрики и исходные расшифровки",
};
const pluralRules = new Intl.PluralRules("ru-RU");
function countLabel(count: number, one: string, few: string, many: string) {
  const category = pluralRules.select(count);
  return category === "one"
    ? one
    : category === "few" || category === "other"
      ? few
      : many;
}
const date = (value: string) =>
  new Date(value).toLocaleDateString("ru-RU", {
    day: "numeric",
    month: "short",
    timeZone: "UTC",
  });
const time = (value: string) =>
  new Date(value).toLocaleString("ru-RU", {
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
    timeZone: "UTC",
  });
const lastDate = computed(() =>
  date(new Date(Date.parse(snapshot.value.to) - 1).toISOString()),
);
watch(
  () => periodConsentKey(props.report, props.connectionKey),
  () => {
    shareText.value = false;
  },
);
watch(
  () => snapshot.value.fingerprint,
  () => {
    showAllTerms.value = false;
  },
);
</script>

<template>
  <section
    class="period-report"
    :class="{ 'period-report--dictations': view === 'trainer-dictations' }"
    aria-label="Периодный речевой отчёт"
    :aria-busy="busy"
  >
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
      Мало данных для общего вывода. ИИ-разбор доступен от 3 диктовок и 100
      слов.
    </p>
    <WlTabs v-model="view" :items="tabs" :pt="tabsPt" class="trainer-tabs" />
    <div
      class="trainer-tab-panel"
      :class="{
        'trainer-tab-panel--dictations': view === 'trainer-dictations',
      }"
      role="tabpanel"
      :aria-labelledby="`wl-tab-${view}`"
      tabindex="0"
    >
      <div v-show="view === 'trainer-overview'" class="overview-layout">
        <WlCard class="overview-chart trainer-card"
          ><DailyChart
            :daily="snapshot.daily"
            :active="view === 'trainer-overview'"
        /></WlCard>
        <WlCard
          class="term-ranking trainer-card"
          role="region"
          aria-label="Рейтинг слов и речевых паттернов"
        >
          <div class="section-header">
            <h3>Чаще встречаются</h3>
            <span class="muted ranking-hint"
              >Нажмите, чтобы открыть диктовки</span
            >
          </div>
          <p v-if="report.stale" class="notice" role="status">
            Рейтинг и связанные диктовки доступны после обновления отчёта.
          </p>
          <template v-else-if="snapshot.terms.length">
            <div class="term-table-heading" aria-hidden="true">
              <span>Слово или паттерн</span><span>Раз</span
              ><span>На 100 слов</span><span>Диктовок</span>
            </div>
            <div class="term-list">
              <button
                v-for="term in terms"
                :key="term.key"
                class="term-row"
                :aria-label="`${term.label}, ${kindLabels[term.kind]}, ${term.count} ${countLabel(term.count, 'вхождение', 'вхождения', 'вхождений')}, ${formatMetric(term.density)} на 100 слов, ${term.affectedSessions} ${countLabel(term.affectedSessions, 'диктовка', 'диктовки', 'диктовок')}`"
                @click="emit('selectTerm', term)"
              >
                <span class="term-label"
                  ><strong>{{ term.label }}</strong
                  ><small>{{ kindLabels[term.kind] }}</small></span
                ><strong>{{ term.count }}</strong
                ><span>{{ formatMetric(term.density) }}</span
                ><span>{{ term.affectedSessions }}</span>
              </button>
            </div>
            <WlButton
              v-if="snapshot.terms.length > 6"
              variant="soft"
              size="sm"
              class="ranking-more"
              @click="showAllTerms = !showAllTerms"
              >{{
                showAllTerms
                  ? "Показать главные"
                  : `Все паттерны · ${snapshot.terms.length}`
              }}</WlButton
            >
          </template>
          <p v-else class="notice">
            {{
              snapshot.includedSessions
                ? "Речевые маркеры в этих диктовках не найдены."
                : "Рейтинг появится после локального разбора записей."
            }}
          </p>
          <p class="muted report-note ranking-note">
            Наблюдения по исходному тексту. Контекст может менять значение
            маркера.
          </p>
        </WlCard>
      </div>
      <section
        v-show="view === 'trainer-dictations'"
        class="trainer-dictations-panel"
        aria-label="Диктовки периода"
      >
        <slot name="dictations" />
      </section>
      <section
        v-show="view === 'trainer-ai'"
        class="period-ai"
        aria-label="ИИ-разбор периода"
      >
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
              shareText
                ? "числовые метрики и выбранные тексты"
                : "числовые метрики"
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
              >{{ report.aiSummary.profileName }} ·
              {{ report.aiSummary.model }} ·
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
    </div>
  </section>
</template>

<style scoped>
.period-report {
  min-width: 0;
}
.period-report--dictations {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}
.period-report--dictations > :not(.trainer-tab-panel) {
  flex: none;
}
.trainer-tab-panel--dictations {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: var(--fono-trainer-workspace-min-height);
}
.trainer-dictations-panel {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}
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
.period-report > .error-text {
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
.trainer-tabs {
  margin-top: var(--fono-space-5);
}
.trainer-tab-panel {
  min-width: 0;
  padding-top: var(--fono-space-5);
}
.trainer-tab-panel:focus-visible {
  outline: 2px solid var(--fono-accent);
  outline-offset: var(--fono-space-1);
  border-radius: var(--fono-radius-sm);
}
.overview-layout {
  display: grid;
  grid-auto-rows: 1fr;
  gap: var(--fono-space-4);
  align-items: stretch;
}
.trainer-card {
  min-width: 0;
  background: var(--fono-surface);
}
.trainer-card :deep(.wl-card__content) {
  padding: var(--fono-space-4);
}
.term-ranking .section-header {
  display: grid;
  gap: var(--fono-space-2);
  margin-bottom: var(--fono-space-4);
}
h3,
h4 {
  margin: 0;
  font-size: var(--fono-type-lg);
}
.ranking-hint {
  font-size: var(--fono-type-sm);
}
.term-table-heading,
.term-row {
  display: grid;
  grid-template-columns:
    minmax(0, 1fr) var(--fono-trainer-ranking-count-width)
    var(--fono-trainer-ranking-density-width) var(
      --fono-trainer-ranking-sessions-width
    );
  gap: var(--fono-space-2);
  align-items: center;
}
.term-table-heading {
  padding: 0 var(--fono-space-3) var(--fono-space-3);
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
}
.term-table-heading > span:not(:first-child) {
  text-align: right;
  white-space: nowrap;
}
.term-row {
  width: 100%;
  padding: var(--fono-space-3);
  border: 0;
  border-bottom: 1px solid var(--fono-border-soft);
  border-radius: var(--fono-radius-sm);
  background: transparent;
  color: var(--fono-text);
  text-align: left;
  font-size: var(--fono-type-md);
}
.term-row:hover {
  background: var(--fono-accent-soft);
}
.term-row:focus-visible {
  outline: 2px solid var(--fono-accent);
  outline-offset: 2px;
}
.term-row > :not(:first-child) {
  text-align: right;
  font-variant-numeric: tabular-nums;
}
.term-label {
  display: grid;
  gap: var(--fono-space-1);
  min-width: 0;
  overflow-wrap: anywhere;
}
.term-label small {
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
}
.ranking-more {
  margin-top: var(--fono-space-3);
}
.report-note {
  font-size: var(--fono-type-md);
  line-height: 1.7;
}
.ranking-note {
  margin-top: var(--fono-space-4);
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
@media (min-width: 1180px) {
  .overview-layout {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
@media (max-width: 780px) {
  .period-report--dictations,
  .trainer-tab-panel--dictations,
  .trainer-dictations-panel {
    flex: 1 0 auto;
    min-height: min-content;
  }
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
