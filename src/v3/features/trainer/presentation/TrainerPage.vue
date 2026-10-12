<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import type { TrainerPeriod, TrainerTerm } from "../domain/report";
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
import SelectField from "../../../shared/presentation/SelectField.vue";
import { PreferenceToggle } from "../../preferences";
import TrainerChart from "./TrainerChart.vue";
import TrainerDetails from "./TrainerDetails.vue";
import PeriodReport from "./PeriodReport.vue";

const workspace = useWorkspace();
const periods: TrainerPeriod[] = [7, 30, 90];
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
const selectedId = ref("");
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
const selected = computed(
  () =>
    sortedEntries.value.find((entry) => entry.id === selectedId.value) ||
    sortedEntries.value[0],
);
function selectTerm(term: TrainerTerm) {
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
  <div class="page">
    <PageHeading
      title="Речевой тренер"
      description="Сравнивайте речевые привычки по диктовкам и выбранному периоду."
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
        <div class="segmented" aria-label="Период анализа">
          <button
            v-for="days in periods"
            :key="days"
            :class="{ selected: periodState.days === days }"
            :aria-pressed="periodState.days === days"
            @click="choosePeriod(days)"
          >
            {{ days }} дней
          </button>
        </div>
        <RouterLink class="text-link" to="/settings/privacy"
          >Хранение данных</RouterLink
        >
      </div>
      <PeriodReport
        v-if="periodState.report"
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
      />
      <div v-else-if="periodState.error" class="notice">
        <p class="error-text" role="alert">{{ periodState.error }}</p>
        <WlButton
          size="sm"
          :disabled="periodState.loading"
          @click="controller.load()"
          >Повторить загрузку отчёта</WlButton
        >
      </div>
      <p v-else class="notice" role="status">Готовим локальный отчёт…</p>
      <div
        v-if="selectedTerm && !periodState.report?.stale"
        class="section-header trainer-filter"
      >
        <span>Диктовки с «{{ selectedTerm.label }}»</span>
        <WlButton size="sm" variant="ghost" @click="selectedTermKey = ''"
          >Показать все диктовки</WlButton
        >
      </div>
      <p v-if="periodState.report?.stale" class="notice" role="status">
        Связанные диктовки появятся после обновления отчёта. Числа выше
        относятся к сохранённому снимку.
      </p>
      <template v-else-if="sortedEntries.length">
        <details class="session-chart">
          <summary>График отдельных диктовок</summary>
          <TrainerChart
            :entries="sortedEntries"
            :selected-id="selected?.id"
            @select="selectedId = $event"
          />
        </details>
        <div class="master-detail">
          <div class="entry-list">
            <div class="trainer-list-toolbar">
              <h3 class="date-label">Разобранные диктовки</h3>
              <SelectField
                v-model="sort"
                label="Сортировка"
                :options="[
                  { value: 'date', label: 'Сначала новые' },
                  { value: 'duration', label: 'Сначала долгие' },
                  { value: 'length', label: 'Сначала длинные тексты' },
                ]"
              />
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
              <p>{{ entry.original }}</p>
              <time :datetime="entry.createdAt">{{
                exactDateLabel(entry.createdAt)
              }}</time>
              <span class="trainer-entry-meta"
                ><span>{{ recordingDurationLabel(entry) }}</span
                ><span>Слов: {{ wordCount(entry) }}</span
                ><span>Маркеров: {{ findingsCount(entry) }}</span></span
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
            : 'Здесь появятся готовые диктовки, включённые в снимок периода. Сохранение истории и локальная аналитика должны быть включены.'
        "
        ><RouterLink to="/settings/privacy" class="text-link"
          >Проверить настройки хранения</RouterLink
        ></EmptyState
      >
    </template>
  </div>
</template>

<style scoped>
.trainer-toolbar {
  flex-wrap: wrap;
  gap: var(--fono-space-3);
}
.trainer-filter {
  flex-wrap: wrap;
  gap: var(--fono-space-3);
  font-size: var(--fono-type-sm);
  margin-bottom: var(--fono-space-4);
}
.session-chart {
  margin-bottom: var(--fono-space-4);
  border: 1px solid var(--fono-border);
  border-radius: var(--fono-radius);
  background: var(--fono-surface);
}
.session-chart summary {
  padding: var(--fono-space-3) var(--fono-space-4);
  cursor: pointer;
  font-size: var(--fono-type-sm);
}
.trainer-list-toolbar {
  padding: var(--fono-space-3);
  border-bottom: 1px solid var(--fono-border);
}
.trainer-list-toolbar .date-label {
  padding: 0;
  margin-bottom: var(--fono-space-3);
}
.trainer-entry strong {
  overflow-wrap: anywhere;
}
.trainer-entry time {
  display: block;
  color: var(--fono-secondary);
  font-size: var(--fono-type-xs);
  font-variant-numeric: tabular-nums;
}
.trainer-entry-meta {
  display: flex;
  flex-wrap: wrap;
  gap: var(--fono-space-1) var(--fono-space-3);
  margin-top: var(--fono-space-1);
  color: var(--fono-muted);
  font-size: var(--fono-type-xs);
}
</style>
