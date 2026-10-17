<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import { WlButton, WlSegmented } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import type { TrainerPeriod, TrainerTerm } from "../domain/report";
import type { TrainerSort } from "../application/overview";
import {
  createPeriodReportController,
  initialPeriodReportState,
} from "../application/periodReportController";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import { PreferenceToggle } from "../../preferences";
import TrainerDictations from "./TrainerDictations.vue";
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
          <TrainerDictations
            v-model:sort="sort"
            v-model:selected-id="selectedId"
            :entries="entries"
            :selected-term="selectedTerm"
            :stale="periodState.report.stale"
            :active="reportView === 'trainer-dictations'"
            @clear-term="selectedTermKey = ''"
          />
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

@media (max-width: 780px) {
  .trainer-page--dictations {
    flex: 1 0 auto;
    min-height: min-content;
  }
}
</style>
