<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed } from "vue";
import { WlCard, WlTabs } from "@whitelife-core/ui-kit";
import type { TrainerReport, TrainerTerm } from "../domain/report";
import DailyChart from "./DailyChart.vue";
import PeriodReportSummary from "./PeriodReportSummary.vue";
import PeriodTerms from "./PeriodTerms.vue";
import PeriodAi from "./PeriodAi.vue";
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
const busy = computed(
  () => props.loading || props.analyzing || props.cancelling,
);
const tabs = computed(() => [
  {
    key: "trainer-overview",
    get label() {
      return uiMessage("ui.bd3a5be11e");
    },
  },
  {
    key: "trainer-dictations",
    get label() {
      return uiMessage("ui.ef9da8f528");
    },
    count: props.report.snapshot.includedSessions,
  },
  {
    key: "trainer-ai",
    label: props.analyzing
      ? uiMessage("ui.2ce793f43d")
      : uiMessage("ui.e9d6ff0a1e"),
  },
]);
// Keep panels mounted: kit tabs unmount inactive content, losing explicit consent.
const tabsPt = {
  tabList: {
    get "aria-label"() {
      return uiMessage("ui.4708392f7b");
    },
  },
  tabpanels: { root: { hidden: true, style: { display: "none" } } },
};
</script>

<template>
  <section
    class="period-report"
    :class="{ 'period-report--dictations': view === 'trainer-dictations' }"
    :aria-label="$t('ui.43e86867ad')"
    :aria-busy="busy"
  >
    <PeriodReportSummary
      :report="report"
      :loading="loading"
      :busy="busy"
      :error="error"
      :native="native"
      @refresh="emit('refresh')"
    />
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
            :daily="report.snapshot.daily"
            :active="view === 'trainer-overview'"
        /></WlCard>

        <PeriodTerms
          :snapshot="report.snapshot"
          :stale="report.stale"
          @select-term="emit('selectTerm', $event)"
        />
      </div>
      <section
        v-show="view === 'trainer-dictations'"
        class="trainer-dictations-panel"
        :aria-label="$t('ui.8eaba588f4')"
      >
        <slot name="dictations" />
      </section>

      <PeriodAi
        v-show="view === 'trainer-ai'"
        :report="report"
        :busy="busy"
        :analyzing="analyzing"
        :cancelling="cancelling"
        :connection-key="connectionKey"
        @analyze="emit('analyze', $event)"
        @cancel="emit('cancel')"
      />
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
}
</style>
