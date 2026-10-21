<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";
import { translateAuthored } from "../../../shared/domain/messages";

import { computed } from "vue";
import type { TrainerDay } from "../domain/report";
import {
  chartScale,
  formatMetric,
  type ChartMetric,
} from "../application/overview";
import {
  chartCategories,
  chartTickIndices,
  dailyChartColumns,
} from "../application/chart";
import { reportDate as dateLabel } from "../application/reportFormat";
const props = defineProps<{
  daily: TrainerDay[];
  metric: ChartMetric;
  selectedIndex: number;
}>();
const emit = defineEmits<{ select: [date: string] }>();
const columns = computed(() => dailyChartColumns(props.daily, props.metric));
const scale = computed(() => chartScale(columns.value.map((day) => day.total)));
const barWidth = computed(() =>
  Math.min(44, (1000 / Math.max(1, props.daily.length)) * 0.72),
);
const ticks = computed(() =>
  chartTickIndices(props.daily.length, props.daily.length <= 7 ? 3 : 5).map(
    (index) => columns.value[index],
  ),
);
const y = (value: number) => 128 - (value / scale.value.max) * 128;
function description(day: TrainerDay): string {
  return uiMessage("ui.0a75fafe7b", {
    v0: String(dateLabel(day.date).replace(/\.$/u, "")),
    v1: String(day.sessions),
    v2: String(day.words),
    v3: String(
      chartCategories
        .map(
          (category) =>
            `${translateAuthored(category.title)}: ${day.counts[category.key]}`,
        )
        .join(". "),
    ),
  });
}
</script>

<template>
  <div v-if="columns.length" class="daily-chart-frame">
    <div class="daily-y-axis" aria-hidden="true">
      <span
        v-for="tick in scale.ticks"
        :key="tick"
        :style="{ bottom: `${(tick / scale.max) * 100}%` }"
        >{{ formatMetric(tick) }}</span
      >
    </div>
    <div class="daily-plot">
      <svg viewBox="0 0 1000 128" preserveAspectRatio="none" aria-hidden="true">
        <path
          v-for="tick in scale.ticks"
          :key="tick"
          :d="`M0 ${y(tick)} H1000`"
          class="daily-grid"
          vector-effect="non-scaling-stroke"
        />
        <g v-for="column in columns" :key="column.day.date">
          <rect
            v-for="segment in column.segments"
            :key="segment.key"
            :x="column.offset * 10 - barWidth / 2"
            :y="y(segment.start + segment.value)"
            :width="barWidth"
            :height="(segment.value / scale.max) * 128"
            :class="'daily-' + segment.key"
          />
          <path
            v-if="column.total === 0"
            :d="`M${column.offset * 10 - barWidth / 2} 127 H${column.offset * 10 + barWidth / 2}`"
            class="daily-zero"
            vector-effect="non-scaling-stroke"
          />
        </g>
      </svg>
      <button
        v-for="(column, index) in columns"
        :key="column.day.date"
        type="button"
        class="daily-column"
        :class="{ selected: selectedIndex === index }"
        :style="{
          left: `${(index * 100) / columns.length}%`,
          width: `${100 / columns.length}%`,
        }"
        tabindex="-1"
        :aria-label="description(column.day)"
        :aria-pressed="selectedIndex === index"
        :title="description(column.day)"
        @click="emit('select', column.day.date)"
      />
    </div>
    <div class="daily-x-axis" aria-hidden="true">
      <span
        v-for="(column, index) in ticks"
        :key="column.day.date"
        :class="{
          first: index === 0,
          last: index === ticks.length - 1 && ticks.length > 1,
          middle: index > 0 && index < ticks.length - 1,
        }"
        :style="{ left: `${column.offset}%` }"
        >{{ dateLabel(column.day.date) }}</span
      >
    </div>
  </div>
  <p v-else class="muted daily-caption">{{ $t("ui.e8a3225ff0") }}</p>
</template>

<style scoped>
.daily-caption {
  margin: 0;
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
  line-height: 1.5;
}
.daily-chart-frame {
  position: relative;
  height: var(--fono-trainer-chart-height);
  flex-shrink: 0;
  margin-top: var(--fono-space-2);
}
.daily-plot {
  position: absolute;
  inset: 0 0 var(--fono-space-6) var(--fono-space-10);
}
.daily-plot svg {
  display: block;
  width: 100%;
  height: 100%;
  overflow: visible;
}
.daily-y-axis {
  position: absolute;
  inset: 0 auto var(--fono-space-6) 0;
  width: var(--fono-space-8);
}
.daily-y-axis span {
  position: absolute;
  right: 0;
  transform: translateY(50%);
}
.daily-x-axis {
  position: absolute;
  inset: auto 0 0 var(--fono-space-10);
  height: var(--fono-space-6);
}
.daily-x-axis span {
  position: absolute;
  top: var(--fono-space-1);
  white-space: nowrap;
}
.daily-x-axis .last {
  transform: translateX(-100%);
}
.daily-x-axis .middle {
  transform: translateX(-50%);
}
.daily-y-axis,
.daily-x-axis {
  color: var(--fono-muted);
  font-size: var(--fono-type-xs);
  font-variant-numeric: tabular-nums;
}
.daily-grid {
  fill: none;
  stroke: var(--fono-border-soft);
}
.daily-zero {
  stroke: var(--fono-muted);
}
.daily-column {
  position: absolute;
  top: 0;
  height: 100%;
  padding: 0;
  border: 0;
  background: transparent;
  cursor: pointer;
}
.daily-column:hover,
.daily-column:focus-visible {
  background: var(--wl-accent-soft);
  outline: 1px solid var(--wl-accent);
  outline-offset: -1px;
}
.daily-column.selected {
  background: var(--wl-accent-soft);
  box-shadow: inset 0 0 0 1px var(--wl-accent-border);
}
.daily-filler {
  fill: var(--fono-trainer-filler);
  background: var(--fono-trainer-filler);
}
.daily-repetition {
  fill: var(--fono-trainer-repetition);
  background: var(--fono-trainer-repetition);
}
.daily-selfCorrection {
  fill: var(--fono-trainer-correction);
  background: var(--fono-trainer-correction);
}
.daily-unfinished {
  fill: var(--fono-trainer-unfinished);
  background: var(--fono-trainer-unfinished);
}
</style>
