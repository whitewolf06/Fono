<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

import { computed, ref } from "vue";
import { WlSegmented } from "@whitelife-core/ui-kit";
import type { TrainerDay } from "../domain/report";
import { type ChartMetric } from "../application/overview";
import {
  chartMetricOptions,
  dailyChartColumns,
  preferredDayIndex,
} from "../application/chart";

const props = withDefaults(
  defineProps<{ daily: TrainerDay[]; active?: boolean }>(),
  { active: true },
);
const metric = ref<ChartMetric>("count");
const selectedDate = ref("");
const columns = computed(() => dailyChartColumns(props.daily, metric.value));
const selectedIndex = computed(() => {
  const index = props.daily.findIndex((day) => day.date === selectedDate.value);
  return index < 0 ? preferredDayIndex(props.daily) : index;
});
const selected = computed(() => columns.value[selectedIndex.value]);
function setMetric(value: string | null) {
  if (value === "count" || value === "density") metric.value = value;
}
function selectDate(value: unknown) {
  if (typeof value === "string") selectedDate.value = value;
}

import DailyChartPlot from "./DailyChartPlot.vue";
import DailyChartBreakdown from "./DailyChartBreakdown.vue";
</script>

<template>
  <section class="period-daily-chart" :aria-label="$t('ui.d53d4db5ee')">
    <div class="daily-toolbar">
      <div class="daily-heading">
        <h3>{{ $t("ui.e91607dca5") }}</h3>
        <p class="daily-caption">
          {{ metric === "density" ? $t("ui.b96b1739e1") : $t("ui.817996b539") }}
        </p>
      </div>
      <WlSegmented
        class="daily-metric-switch"
        :model-value="metric"
        :options="chartMetricOptions"
        :aria-label="$t('ui.dd9a8af00e')"
        @update:model-value="setMetric"
      />
    </div>

    <DailyChartPlot
      :daily="daily"
      :metric="metric"
      :selected-index="selectedIndex"
      @select="selectDate"
    />
    <DailyChartBreakdown
      :daily="daily"
      :selected="selected"
      :metric="metric"
      :active="active"
      @select="selectDate"
    />
  </section>
</template>

<style scoped>
.period-daily-chart {
  display: flex;
  flex-direction: column;
  gap: var(--fono-space-4);
  min-width: 0;
  container-type: inline-size;
  container-name: trainer-daily;
}
.daily-toolbar {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: var(--fono-space-3);
}
.daily-heading {
  display: flex;
  flex: 1 1 var(--fono-trainer-day-select-width);
  flex-direction: column;
  gap: var(--fono-space-1);
  min-width: 0;
}
.daily-toolbar h3 {
  margin: 0;
  font-size: var(--fono-type-lg);
}
.daily-caption {
  margin: 0;
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
  line-height: 1.5;
}
.daily-metric-switch {
  flex-shrink: 0;
}
.daily-metric-switch :deep(.wl-segmented__item) {
  font-size: var(--fono-type-md);
}
.daily-metric-switch :deep(.wl-segmented__item.is-active) {
  color: var(--wl-action-primary-text);
  background: var(--wl-action-primary-bg);
  box-shadow: none;
}
.daily-metric-switch :deep(.wl-segmented__item.is-active:hover) {
  background: var(--wl-action-primary-hover);
}
.daily-metric-switch :deep(.wl-segmented__item:focus-visible) {
  outline: none;
  box-shadow: var(--wl-focus-ring);
}
</style>
