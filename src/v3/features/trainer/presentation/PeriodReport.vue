<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { WlButton, WlCheckbox } from "@whitelife-core/ui-kit";
import type { TrainerReport, TrainerTerm } from "../domain/report";
import {
  periodAiDisabledReason,
  periodConsentKey,
} from "../application/periodReportController";
import { formatMetric } from "../application/overview";
import DailyChart from "./DailyChart.vue";

const props = defineProps<{
  report: TrainerReport;
  loading: boolean;
  analyzing: boolean;
  cancelling: boolean;
  error: string;
  native: boolean;
  connectionKey?: string;
}>();
const emit = defineEmits<{
  refresh: [];
  analyze: [shareText: boolean];
  cancel: [];
  selectTerm: [term: TrainerTerm];
}>();
const shareText = ref(false);
const snapshot = computed(() => props.report.snapshot);
const configuration = computed(() => props.report.aiConfiguration);
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
    month: "long",
    year: "numeric",
    timeZone: "UTC",
  });
const time = (value: string) =>
  new Date(value).toLocaleString("ru-RU", {
    day: "numeric",
    month: "short",
    year: "numeric",
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
</script>

<template>
  <section
    class="period-report"
    aria-label="Периодный речевой отчёт"
    :aria-busy="busy"
  >
    <div class="section-header report-heading">
      <div>
        <small class="eyebrow">Отчёт за {{ snapshot.days }} дней</small>
        <h2>{{ date(snapshot.from) }} — {{ lastDate }}</h2>
        <p class="muted report-note">Дни считаются по UTC.</p>
      </div>
      <WlButton
        size="sm"
        variant="ghost"
        :loading="loading"
        :disabled="busy || (!report.stale && !error)"
        @click="emit('refresh')"
        >{{
          error && !report.stale ? "Повторить проверку" : "Обновить отчёт"
        }}</WlButton
      >
    </div>
    <p v-if="!native" class="notice report-note">
      Предпросмотр использует демонстрационные записи и ответы ИИ.
    </p>
    <p
      class="report-freshness"
      :class="{ stale: report.stale, checking: loading }"
      role="status"
    >
      {{
        loading
          ? "Проверяем состав данных…"
          : report.stale
            ? "Данные изменились. Ниже сохранённый снимок; обновите отчёт для нового разбора."
            : "Отчёт соответствует текущим данным."
      }}
      <span>Снимок: {{ time(snapshot.generatedAt) }} UTC.</span>
    </p>
    <div class="report-counts">
      <div>
        <strong>{{ snapshot.includedSessions }}</strong
        ><span>В отчёте</span>
      </div>
      <div>
        <strong>{{ snapshot.excludedSessions }}</strong
        ><span>Исключено</span>
      </div>
      <div>
        <strong>{{ snapshot.pendingSessions }}</strong
        ><span>Ждут локального разбора</span>
      </div>
      <div>
        <strong>{{ snapshot.unavailableSessions }}</strong
        ><span>Нет данных для разбора</span>
      </div>
    </div>
    <p class="muted report-note">
      Проверяются последние {{ snapshot.historyLimit }} записей архива. Исходные
      расшифровки хранятся до {{ snapshot.retentionDays }} дней.
      <template v-if="snapshot.coverageStart">
        Доступные записи — с {{ date(snapshot.coverageStart) }}
      </template>
      Более старые или несохранённые расшифровки в отчёт не входят.
    </p>
    <p v-if="!snapshot.includedSessions" class="notice">
      В этом периоде пока нет готовых диктовок, включённых в аналитику.
      Проверьте ожидающие и исключённые записи, а также настройки хранения.
    </p>
    <p v-else-if="snapshot.insufficientData" class="notice">
      Данных мало. Для периодного ИИ-разбора нужны хотя бы 3 диктовки и 100
      слов. Локальные числа доступны, но по небольшой выборке нельзя судить об
      общей привычке.
    </p>
    <div class="report-metrics">
      <span
        ><strong>{{ snapshot.totalWords }}</strong> слов</span
      >
      <span
        ><strong>{{ markerCount }}</strong> маркеров</span
      >
      <span
        ><strong>{{ formatMetric(snapshot.density) }}</strong> на 100 слов</span
      >
    </div>
    <DailyChart :daily="snapshot.daily" />
    <section class="term-ranking" aria-label="Рейтинг слов и речевых паттернов">
      <div class="section-header">
        <h3>Чаще встречаются</h3>
        <small class="muted"
          >Правила анализа: {{ snapshot.rulesVersion }}</small
        >
      </div>
      <p class="muted report-note">
        Это наблюдения по исходному тексту, а не оценка правильности речи.
        <template v-if="!report.stale">
          Выберите слово или паттерн, чтобы увидеть связанные диктовки.
        </template>
      </p>
      <p v-if="report.stale" class="notice" role="status">
        Рейтинг с примерами и связанные диктовки доступны после обновления
        отчёта. Числовая сводка выше относится к сохранённому снимку.
      </p>
      <div v-else-if="snapshot.terms.length" class="term-list">
        <button
          v-for="term in snapshot.terms"
          :key="term.key"
          class="term-row"
          @click="emit('selectTerm', term)"
        >
          <span class="term-label"
            >{{ term.label }}<small>{{ kindLabels[term.kind] }}</small></span
          >
          <span
            ><strong>{{ term.count }}</strong
            ><small>{{
              countLabel(term.count, "вхождение", "вхождения", "вхождений")
            }}</small></span
          >
          <span
            ><strong>{{ formatMetric(term.density) }}</strong
            ><small>на 100 слов</small></span
          >
          <span
            ><strong>{{ term.affectedSessions }}</strong
            ><small>{{
              countLabel(
                term.affectedSessions,
                "диктовка",
                "диктовки",
                "диктовок",
              )
            }}</small></span
          >
        </button>
      </div>
      <p v-else class="notice">
        {{
          snapshot.includedSessions
            ? "В учтённых диктовках речевые маркеры не найдены."
            : "Рейтинг появится после локального разбора подходящих записей."
        }}
      </p>
    </section>
    <section class="period-ai" aria-label="ИИ-разбор периода">
      <h3>Разбор периода через ИИ</h3>
      <p class="muted report-note">
        Разбор запускается только этой кнопкой. По умолчанию выбранное
        подключение получает числовые метрики и границы периода.
      </p>
      <p class="report-profile">
        Подключение: {{ configuration.profileName || "не выбрано" }} · модель:
        {{ configuration.model || "не выбрана" }}
      </p>
      <div
        v-if="configuration.dataScope !== 'metrics_only'"
        class="report-sharing"
      >
        <WlCheckbox v-model="shareText" :disabled="busy">
          {{
            configuration.dataScope === "original_text"
              ? "Также передать исходные расшифровки учтённых диктовок"
              : "Также передать примеры найденных речевых маркеров"
          }}
        </WlCheckbox>
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
        будут переданы внешнему провайдеру.
        <template v-if="!configuration.cloudConsent">
          Сначала разрешите передачу данных в настройках ИИ.
        </template>
      </p>
      <div class="actions report-actions">
        <WlButton
          size="sm"
          :loading="analyzing"
          :disabled="busy || !!blocked"
          @click="emit('analyze', shareText)"
          >Проанализировать период</WlButton
        >
        <WlButton
          v-if="analyzing || cancelling"
          size="sm"
          variant="ghost"
          :loading="cancelling"
          :disabled="cancelling"
          @click="emit('cancel')"
          >Отменить разбор</WlButton
        >
        <RouterLink
          class="text-link"
          to="/settings/processing?field=trainerModel"
          >Настройки ИИ</RouterLink
        >
      </div>
      <p v-if="blocked" class="muted report-note" role="status">
        {{ blocked }}
      </p>
      <p v-if="error" class="error-text" role="alert">{{ error }}</p>
      <article v-if="report.aiSummary" class="period-ai-result">
        <p v-if="report.aiStale" class="notice">
          Этот ИИ-разбор относится к прежним данным или настройкам.
        </p>
        <div class="section-header report-heading">
          <h4>Сохранённый разбор</h4>
          <small class="muted">
            {{ report.aiSummary.profileName }} · {{ report.aiSummary.model }} ·
            {{ time(report.aiSummary.generatedAt) }} UTC
          </small>
        </div>
        <p class="summary-text">{{ report.aiSummary.summary }}</p>
        <p class="muted report-note">
          Данные этого разбора: {{ scopeLabels[report.aiSummary.dataScope] }}.
        </p>
        <ol class="period-priorities">
          <li
            v-for="(priority, index) in report.aiSummary.priorities"
            :key="index"
          >
            <strong>{{ priority.title }}</strong>
            <p><span class="muted">Основание:</span> {{ priority.basis }}</p>
            <p><span class="muted">Практика:</span> {{ priority.exercise }}</p>
          </li>
        </ol>
      </article>
    </section>
  </section>
</template>

<style scoped>
.period-report {
  padding: var(--fono-space-5);
  margin-bottom: var(--fono-space-5);
  border: 1px solid var(--fono-border);
  border-radius: var(--fono-radius);
  background: var(--fono-surface);
}
.report-heading {
  align-items: flex-start;
  flex-wrap: wrap;
  gap: var(--fono-space-3);
}
.report-heading h2,
.report-heading h4 {
  margin: var(--fono-space-1) 0;
  font-size: var(--fono-type-lg);
  overflow-wrap: anywhere;
}
.report-note,
.report-profile,
.report-freshness {
  font-size: var(--fono-type-sm);
  line-height: 1.6;
}
.report-freshness {
  display: grid;
  gap: var(--fono-space-1);
  color: var(--fono-success);
}
.report-freshness span {
  color: var(--fono-muted);
}
.report-freshness.stale {
  color: var(--fono-warning);
}
.report-freshness.checking {
  color: var(--fono-muted);
}
.report-counts {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: var(--fono-space-3);
  margin: var(--fono-space-4) 0;
}
.report-counts div {
  display: grid;
  gap: var(--fono-space-1);
  padding: var(--fono-space-3);
  border: 1px solid var(--fono-border-soft);
  border-radius: var(--fono-radius-sm);
}
.report-counts strong,
.report-metrics strong {
  color: var(--fono-electric-soft);
  font-size: var(--fono-type-xl);
  font-variant-numeric: tabular-nums;
}
.report-counts span {
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
}
.report-metrics {
  display: flex;
  flex-wrap: wrap;
  gap: var(--fono-space-3) var(--fono-space-5);
  margin: var(--fono-space-5) 0;
  font-size: var(--fono-type-sm);
}
.term-ranking,
.period-ai {
  margin-top: var(--fono-space-6);
  padding-top: var(--fono-space-4);
  border-top: 1px solid var(--fono-border);
}
.term-ranking .section-header {
  flex-wrap: wrap;
  gap: var(--fono-space-2);
}
h3 {
  margin: 0;
  font-size: var(--fono-type-lg);
}
.term-list {
  display: grid;
  gap: var(--fono-space-2);
}
.term-row {
  display: grid;
  grid-template-columns: minmax(0, 2fr) repeat(3, minmax(0, 1fr));
  align-items: center;
  gap: var(--fono-space-3);
  width: 100%;
  padding: var(--fono-space-3);
  border: 1px solid var(--fono-border-soft);
  border-radius: var(--fono-radius-sm);
  background: var(--fono-surface-raised);
  color: var(--fono-text);
  text-align: left;
}
.term-row:hover {
  border-color: var(--fono-accent);
}
.term-row:focus-visible {
  outline: 2px solid var(--fono-accent);
  outline-offset: 2px;
}
.term-row > span {
  display: grid;
  gap: var(--fono-space-1);
  min-width: 0;
  overflow-wrap: anywhere;
}
.term-row small {
  color: var(--fono-muted);
  font-size: var(--fono-type-xs);
}
.term-label {
  font-size: var(--fono-type-lg);
}
.report-sharing {
  display: grid;
  gap: var(--fono-space-1);
  margin: var(--fono-space-4) 0;
}
.report-actions {
  flex-wrap: wrap;
  gap: var(--fono-space-3);
}
.period-ai-result {
  margin-top: var(--fono-space-4);
  padding: var(--fono-space-4);
  border: 1px solid var(--fono-border-soft);
  border-radius: var(--fono-radius-sm);
  background: var(--fono-surface-raised);
}
.summary-text,
.period-priorities p {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.period-priorities {
  padding-left: var(--fono-space-5);
}
.period-priorities li + li {
  margin-top: var(--fono-space-4);
}
@media (max-width: 780px) {
  .period-report {
    padding: var(--fono-space-3);
  }
  .report-counts {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .term-row {
    gap: var(--fono-space-2);
  }
}
</style>
