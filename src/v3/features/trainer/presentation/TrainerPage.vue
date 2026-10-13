<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import {
  WlButton,
  WlField,
  WlSelect,
  WlSegmented,
} from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import type { TrainerPeriod, TrainerTerm } from "../domain/report";
import { analyze } from "../domain/analysis";
import { transcriptHighlights } from "../application/transcriptHighlights";
import {
  exactDateLabel,
  findingsCount,
  recordingDurationLabel,
  sortTrainerEntries,
  wordCount,
  type TrainerSort,
} from "../application/overview";
import {
  createPeriodReportController,
  initialPeriodReportState,
} from "../application/periodReportController";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import EmptyState from "../../../shared/presentation/EmptyState.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import { PreferenceToggle } from "../../preferences";
import TrainerDetails from "./TrainerDetails.vue";
import PeriodReport from "./PeriodReport.vue";

const workspace = useWorkspace();
const periods: TrainerPeriod[] = [7, 30, 90];
const periodOptions = periods.map((days) => ({
  value: String(days),
  label: `${days} дней`,
}));
const periodState = reactive(initialPeriodReportState());
const controller = createPeriodReportController(workspace.trainer, periodState);
const connectionKey = computed(() => {
  const preferences = workspace.state.preferences;
  const profile = workspace.state.profiles.find(
    (entry) => entry.id === preferences.trainerProfile,
  );
  return JSON.stringify([
    preferences.trainerEnabled,
    preferences.analyticsConsent,
    preferences.retentionDays,
    preferences.trainerAiEnabled,
    preferences.trainerProfile,
    preferences.trainerModel,
    preferences.trainerScope,
    preferences.cloudConsent,
    profile?.id,
    profile?.url,
    profile?.provider,
    profile?.location,
    profile?.model,
    profile?.name,
  ]);
});
let stopDayChange = () => {};
const sort = ref<TrainerSort>("date");
const sortOptions = [
  { value: "date", label: "Сначала новые" },
  { value: "duration", label: "Сначала долгие" },
  { value: "length", label: "Сначала длинные тексты" },
];
const selectedId = ref("");
const reportView = ref("trainer-overview");
const selectedTermKey = ref("");
const selectedTerm = computed(() =>
  periodState.report?.snapshot.terms.find(
    (term) => term.key === selectedTermKey.value,
  ),
);
const entries = computed(() =>
  workspace.state.history.filter(
    (entry) =>
      entry.analyticsIncluded !== false &&
      entry.original &&
      (!workspace.native || entry.analysisReady) &&
      periodState.report?.snapshot.sessionIds.includes(entry.id),
  ),
);
const sortedEntries = computed(() =>
  sortTrainerEntries(
    selectedTerm.value
      ? entries.value.filter((entry) =>
          selectedTerm.value!.sessionIds.includes(entry.id),
        )
      : entries.value,
    sort.value,
  ),
);
const entryHighlights = computed(
  () =>
    new Map(
      sortedEntries.value.map((entry) => [
        entry.id,
        transcriptHighlights(entry.original || "", analyze(entry)),
      ]),
    ),
);
const selected = computed(
  () =>
    sortedEntries.value.find((entry) => entry.id === selectedId.value) ||
    sortedEntries.value[0],
);
function selectTerm(term: TrainerTerm) {
  reportView.value = "trainer-dictations";
  selectedTermKey.value = term.key;
  selectedId.value =
    term.sessionIds.find((id) =>
      entries.value.some((entry) => entry.id === id),
    ) || "";
}
async function choosePeriod(days: TrainerPeriod) {
  selectedTermKey.value = "";
  selectedId.value = "";
  await controller.setPeriod(days);
}
function choosePeriodValue(value: string | null) {
  const days = Number(value) as TrainerPeriod;
  if (periods.includes(days)) void choosePeriod(days);
}
function chooseSort(value: unknown) {
  if (value === "date" || value === "duration" || value === "length")
    sort.value = value;
}
watch(
  () =>
    JSON.stringify([
      workspace.state.history.map((entry) => [
        entry.id,
        entry.createdAt,
        entry.analyticsIncluded,
        entry.original,
        entry.analysisReady,
        entry.findings,
      ]),
      workspace.state.preferences.trainerEnabled,
      workspace.state.preferences.analyticsConsent,
      workspace.state.preferences.retentionDays,
      workspace.state.preferences.trainerAiEnabled,
      workspace.state.preferences.trainerProfile,
      workspace.state.preferences.trainerModel,
      workspace.state.preferences.trainerScope,
      workspace.state.preferences.cloudConsent,
      workspace.state.profiles.map((profile) => [
        profile.id,
        profile.name,
        profile.model,
        profile.url,
        profile.provider,
        profile.location,
        profile.hasApiKey,
      ]),
    ]),
  () => {
    if (workspace.state.preferences.trainerEnabled) void controller.load();
    else void controller.clear();
  },
  { immediate: true },
);
onMounted(() => {
  stopDayChange =
    workspace.trainer.onDayChange?.(() => {
      if (workspace.state.preferences.trainerEnabled) void controller.load();
    }) || (() => {});
});
onUnmounted(() => {
  stopDayChange();
  void controller.dispose();
});
</script>

<template>
  <div
    class="page trainer-page"
    :class="{
      'trainer-page--dictations':
        reportView === 'trainer-dictations' &&
        workspace.state.preferences.trainerEnabled &&
        !!periodState.report,
    }"
  >
    <PageHeading
      title="Речевой тренер"
      description="Речевые привычки, динамика и разбор диктовок."
      ><PreferenceToggle name="trainerEnabled" label="Речевой тренер" compact
    /></PageHeading>
    <div
      v-if="!workspace.state.preferences.trainerEnabled"
      class="welcome-panel"
    >
      <div class="large-icon"><AppIcon name="activity" :size="36" /></div>
      <h2>Услышать себя по-новому</h2>
      <p>
        Fono замечает слова-паразиты, повторы и самопоправки в исходных
        расшифровках. Анализ остаётся на вашем компьютере.
      </p>
      <PreferenceToggle
        name="trainerEnabled"
        label="Включить речевого тренера"
        description="Потребуется разрешение сохранять исходные расшифровки."
      />
      <small v-if="!workspace.native"
        >Предпросмотр использует демонстрационные записи.</small
      >
    </div>
    <template v-else>
      <div class="section-header trainer-toolbar">
        <WlSegmented
          :model-value="String(periodState.days)"
          :options="periodOptions"
          :pt="{ root: { 'aria-label': 'Период анализа' } }"
          @update:model-value="choosePeriodValue"
        />
        <RouterLink class="text-link" to="/settings/privacy"
          >Хранение данных</RouterLink
        >
      </div>
      <PeriodReport
        v-if="periodState.report"
        v-model:view="reportView"
        :report="periodState.report"
        :loading="periodState.loading"
        :analyzing="periodState.analyzing"
        :cancelling="periodState.cancelling"
        :error="periodState.error"
        :native="!!workspace.native"
        :connection-key="connectionKey"
        @refresh="controller.load(true)"
        @analyze="controller.analyze"
        @cancel="controller.cancel"
        @select-term="selectTerm"
      >
        <template #dictations>
          <div
            v-if="selectedTerm && !periodState.report?.stale"
            class="section-header trainer-filter"
          >
            <span
              >Маркер «{{ selectedTerm.label }}» · {{ sortedEntries.length }} из
              {{ entries.length }} диктовок</span
            >
            <WlButton size="sm" variant="soft" @click="selectedTermKey = ''"
              >Показать все диктовки</WlButton
            >
          </div>
          <p v-if="periodState.report?.stale" class="notice" role="status">
            Обновите отчёт, чтобы увидеть диктовки текущего периода. Числа выше
            относятся к сохранённому снимку.
          </p>
          <template v-else-if="sortedEntries.length">
            <div class="master-detail trainer-master-detail">
              <div class="entry-list trainer-entry-list">
                <div class="trainer-list-toolbar">
                  <h3>Диктовки периода</h3>
                  <WlField v-slot="field" label="Сортировка">
                    <WlSelect
                      v-if="reportView === 'trainer-dictations'"
                      v-bind="field"
                      :model-value="sort"
                      :options="sortOptions"
                      option-label="label"
                      option-value="value"
                      size="md"
                      aria-label="Сортировка"
                      @update:model-value="chooseSort"
                    />
                  </WlField>
                </div>
                <button
                  v-for="entry in sortedEntries"
                  :key="entry.id"
                  class="entry-button trainer-entry"
                  :class="{ selected: selected?.id === entry.id }"
                  :aria-pressed="selected?.id === entry.id"
                  @click="selectedId = entry.id"
                >
                  <strong>{{ entry.title }}</strong>
                  <p>
                    <template
                      v-for="(segment, index) in entryHighlights.get(entry.id)"
                      :key="index"
                      ><mark
                        v-if="segment.highlighted"
                        class="entry-marker"
                        :data-category="segment.category"
                        >{{ segment.text }}</mark
                      ><span v-else>{{ segment.text }}</span></template
                    >
                  </p>
                  <time :datetime="entry.createdAt">{{
                    exactDateLabel(entry.createdAt)
                  }}</time>
                  <span class="trainer-entry-meta"
                    ><span>{{ recordingDurationLabel(entry) }}</span
                    ><span>{{ wordCount(entry) }} слов</span
                    ><span>{{ findingsCount(entry) }} маркеров</span></span
                  >
                </button>
              </div>
              <TrainerDetails v-if="selected" :entry="selected" />
            </div>
          </template>
          <EmptyState
            v-else
            :title="
              selectedTerm
                ? 'Подходящие диктовки сейчас недоступны'
                : 'Пока нечего разбирать'
            "
            :text="
              selectedTerm
                ? 'Записи могли измениться или быть исключены. Обновите отчёт либо покажите все диктовки.'
                : 'Нужны сохранённые исходные расшифровки и завершённый локальный разбор. Записи с одним итоговым текстом остаются в общей истории.'
            "
            ><RouterLink to="/history" class="text-link"
              >Открыть историю</RouterLink
            ></EmptyState
          >
        </template>
      </PeriodReport>
      <div v-else-if="periodState.error" class="notice">
        <p class="error-text" role="alert">{{ periodState.error }}</p>
        <WlButton
          size="sm"
          variant="primary"
          :disabled="periodState.loading"
          @click="controller.load()"
          >Повторить загрузку отчёта</WlButton
        >
      </div>
      <p v-else class="notice" role="status">Готовим локальный отчёт…</p>
    </template>
  </div>
</template>

<style scoped>
.trainer-page--dictations {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}
.trainer-page--dictations > :not(.period-report) {
  flex: none;
}
.trainer-master-detail {
  flex: 1;
  min-height: 0;
  grid-template-rows: minmax(0, 1fr);
}
.trainer-master-detail :deep(.trainer-detail) {
  min-height: 0;
  overflow: auto;
}
.trainer-master-detail :deep(.trainer-original-text) {
  max-height: none;
  overflow: visible;
}
.trainer-toolbar {
  flex-wrap: wrap;
  gap: var(--fono-space-3);
  margin-bottom: var(--fono-space-4);
}
.trainer-toolbar :deep(.wl-segmented__item[aria-pressed="true"]) {
  background: var(--wl-action-primary-bg);
  color: var(--wl-action-primary-text);
}
.trainer-toolbar :deep(.wl-segmented__item[aria-pressed="true"]:hover) {
  background: var(--wl-action-primary-hover);
}
.trainer-filter {
  flex: none;
  flex-wrap: wrap;
  gap: var(--fono-space-3);
  font-size: var(--fono-type-sm);
  margin-bottom: var(--fono-space-3);
  padding: var(--fono-space-2) var(--fono-space-3);
  background: var(--fono-accent-soft);
  border-radius: var(--fono-radius);
}
.trainer-list-toolbar {
  display: grid;
  gap: var(--fono-space-4);
  padding: var(--fono-space-4);
  border-bottom: 1px solid var(--fono-border);
}
.trainer-list-toolbar h3 {
  font-size: var(--fono-type-lg);
  margin: 0;
}
.trainer-entry-list {
  min-height: 0;
  max-height: none;
}
.trainer-entry {
  padding: var(--fono-space-4);
}
.trainer-entry strong {
  overflow-wrap: anywhere;
  font-size: var(--fono-type-md);
}
.entry-marker {
  color: var(--marker-color);
  background: transparent;
  text-decoration-line: underline;
  text-decoration-thickness: from-font;
  text-underline-offset: 0.15em;
}
.entry-marker[data-category="filler"] {
  --marker-color: var(--fono-trainer-filler);
}
.entry-marker[data-category="repetition"] {
  --marker-color: var(--fono-trainer-repetition);
}
.entry-marker[data-category="selfCorrection"] {
  --marker-color: var(--fono-trainer-correction);
}
.entry-marker[data-category="unfinished"] {
  --marker-color: var(--fono-trainer-unfinished);
}
.trainer-entry time {
  display: block;
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
  font-variant-numeric: tabular-nums;
}
.trainer-entry-meta {
  display: flex;
  flex-wrap: wrap;
  gap: var(--fono-space-1) var(--fono-space-3);
  margin-top: var(--fono-space-2);
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
}
@media (max-width: 780px) {
  .trainer-page--dictations {
    flex: 1 0 auto;
    min-height: min-content;
  }
  .trainer-master-detail {
    flex: 1 0 auto;
    grid-template-columns: 1fr;
    grid-template-rows: max-content minmax(min-content, 1fr);
  }
  .trainer-master-detail :deep(.trainer-detail) {
    overflow: visible;
  }
  .trainer-entry-list {
    max-height: var(--fono-trainer-entry-list-compact-height);
    border-right: 0;
    border-bottom: 1px solid var(--fono-border);
  }
}
</style>
