<script setup lang="ts">
import { computed, ref } from "vue";
import { WlField, WlSegmented, WlSelect } from "@whitelife-core/ui-kit";
import type { TrainerDay } from "../domain/report";
import {
  chartScale,
  formatMetric,
  type ChartMetric,
} from "../application/overview";
import {
  chartCategories,
  chartMetricOptions,
  chartTickIndices,
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
const scale = computed(() => chartScale(columns.value.map((day) => day.total)));
const barWidth = computed(() =>
  Math.min(44, (1000 / Math.max(1, props.daily.length)) * 0.72),
);
const ticks = computed(() =>
  chartTickIndices(props.daily.length, props.daily.length <= 7 ? 3 : 5).map(
    (index) => columns.value[index],
  ),
);
const selectedIndex = computed(() => {
  const index = props.daily.findIndex((day) => day.date === selectedDate.value);
  return index < 0 ? preferredDayIndex(props.daily) : index;
});
const selected = computed(() => columns.value[selectedIndex.value]);
const dateOptions = computed(() =>
  props.daily.map((day) => ({ label: dateLabel(day.date), value: day.date })),
);
const y = (value: number) => 128 - (value / scale.value.max) * 128;
function setMetric(value: string | null) {
  if (value === "count" || value === "density") metric.value = value;
}
function selectDate(value: unknown) {
  if (typeof value === "string") selectedDate.value = value;
}
const dateLabel = (date: string) =>
  new Date(date).toLocaleDateString("ru-RU", {
    day: "numeric",
    month: "short",
    timeZone: "UTC",
  });
function description(day: TrainerDay): string {
  return `${dateLabel(day.date).replace(/\.$/u, "")}. Диктовок: ${day.sessions}. Слов: ${day.words}. ${chartCategories.map((category) => `${category.title}: ${day.counts[category.key]}`).join(". ")}.`;
}
</script>

<template>
  <section class="period-daily-chart" aria-label="Речевые маркеры по дням">
    <div class="daily-toolbar">
      <div class="daily-heading">
        <h3>По дням</h3>
        <p class="daily-caption">
          {{
            metric === "density"
              ? "Маркеры на 100 слов исходного текста, по дням UTC."
              : "Маркеры в исходных расшифровках, по дням UTC."
          }}
        </p>
      </div>
      <WlSegmented
        class="daily-metric-switch"
        :model-value="metric"
        :options="chartMetricOptions"
        aria-label="Единица дневного графика"
        @update:model-value="setMetric"
      />
    </div>
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
        <svg
          viewBox="0 0 1000 128"
          preserveAspectRatio="none"
          aria-hidden="true"
        >
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
          @click="selectedDate = column.day.date"
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
    <p v-else class="muted daily-caption">В снимке нет дней для графика.</p>
    <div v-if="selected" class="daily-breakdown">
      <div class="daily-selected-heading">
        <WlField
          v-if="active"
          v-slot="field"
          class="daily-day-field"
          label="Выбранный день"
        >
          <WlSelect
            v-bind="field"
            :model-value="selected.day.date"
            :options="dateOptions"
            option-label="label"
            option-value="value"
            size="sm"
            density="compact"
            aria-label="Выбранный день"
            @update:model-value="selectDate"
          />
        </WlField>
        <dl class="daily-metrics" aria-live="polite" aria-atomic="true">
          <div>
            <dt>Диктовок</dt>
            <dd>{{ selected.day.sessions }}</dd>
          </div>
          <div>
            <dt>Слов</dt>
            <dd>{{ selected.day.words }}</dd>
          </div>
          <div>
            <dt>{{ metric === "density" ? "На 100 слов" : "Маркеров" }}</dt>
            <dd>{{ formatMetric(selected.total) }}</dd>
          </div>
        </dl>
      </div>
      <dl
        class="daily-legend"
        aria-label="Маркеры выбранного дня по категориям"
      >
        <div v-for="segment in selected.segments" :key="segment.key">
          <dt>
            <i :class="'daily-' + segment.key" aria-hidden="true" />
            <span>{{ segment.title }}</span>
          </dt>
          <dd>{{ formatMetric(segment.value) }}</dd>
        </div>
      </dl>
      <p v-if="selected.day.sessions === 0" class="muted daily-caption">
        За этот день в снимке нет учтённых диктовок.
      </p>
      <p
        v-else-if="selected.segments.every((segment) => segment.count === 0)"
        class="muted daily-caption"
      >
        В учтённых диктовках этого дня маркеры не найдены.
      </p>
    </div>
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
.daily-breakdown {
  display: flex;
  flex-direction: column;
  gap: var(--fono-space-2);
}
.daily-selected-heading {
  display: flex;
  align-items: flex-end;
  flex-wrap: wrap;
  gap: var(--fono-space-2) var(--fono-space-3);
}
.daily-day-field {
  flex: 0 1 var(--fono-trainer-day-select-width);
  width: var(--fono-trainer-day-select-width);
  min-width: 0;
}
.daily-day-field :deep(.wl-select) {
  color: var(--fono-text);
  font-size: var(--fono-type-md);
  background: var(--fono-surface-raised);
}
.daily-day-field :deep(.wl-field__label) {
  font-size: var(--fono-type-md);
}
.daily-metrics {
  display: grid;
  flex: 1 1 auto;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: var(--fono-space-3);
  min-width: calc(var(--fono-trainer-day-select-width) + var(--fono-space-8));
  margin: 0;
}
.daily-metrics dt {
  margin-bottom: var(--fono-space-1);
  color: var(--fono-secondary);
  font-size: var(--fono-type-md);
}
.daily-metrics dd {
  margin: 0;
  color: var(--fono-text);
  font-size: var(--fono-type-xl);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
.daily-legend {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--fono-space-1) var(--fono-space-4);
  margin: 0;
  color: var(--fono-secondary);
  font-size: var(--fono-type-md);
  line-height: 1.5;
}
.daily-legend > div {
  display: flex;
  align-items: center;
  gap: var(--fono-space-2);
  min-width: 0;
}
.daily-legend dt {
  display: flex;
  align-items: center;
  gap: var(--fono-space-2);
  min-width: 0;
}
.daily-legend dd {
  flex-shrink: 0;
  margin: 0;
  color: var(--fono-text);
  font-size: var(--fono-type-lg);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
.daily-legend dt span {
  overflow-wrap: anywhere;
}
.daily-legend i {
  flex-shrink: 0;
  width: var(--fono-space-2);
  height: var(--fono-space-2);
  border-radius: var(--fono-radius-sm);
}
@container trainer-daily (min-width: 720px) {
  .daily-legend {
    grid-template-columns: repeat(4, minmax(0, 1fr));
  }
}
</style>
