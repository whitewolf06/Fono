<script setup lang="ts">
import { computed, ref } from "vue";
import type { Dictation } from "../../../shared/domain/contracts";
import { analyze, findingTitles } from "../domain/analysis";
import {
  chartEntries,
  chartScale,
  chartValue,
  exactDateLabel,
  findingsCount,
  formatMetric,
  type ChartMetric,
} from "../application/overview";

const props = defineProps<{ entries: Dictation[]; selectedId?: string }>();
const emit = defineEmits<{ select: [id: string] }>();
const metric = ref<ChartMetric>("count");
const visible = computed(() => chartEntries(props.entries));
const columns = computed(() =>
  visible.value.map((entry, i) => {
    let accumulated = 0;
    const segments = analyze(entry).map((finding) => {
      const value = chartValue(finding.count, entry, metric.value);
      const result = { ...finding, value, start: accumulated };
      accumulated += value;
      return result;
    });
    return {
      entry,
      x: 46 + (i + 0.5) * (440 / visible.value.length),
      value: accumulated,
      segments,
    };
  }),
);
const scale = computed(() =>
  chartScale(columns.value.map((column) => column.value)),
);
const barWidth = computed(() =>
  Math.min(42, (440 / Math.max(1, columns.value.length)) * 0.7),
);
const y = (value: number) => 164 - (value / scale.value.max) * 132;
const categoryClass = (title: string) =>
  `category-${findingTitles.indexOf(title as (typeof findingTitles)[number])}`;
const shortDate = (date: string) =>
  new Date(date).toLocaleString("ru-RU", {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
function description(entry: Dictation): string {
  return `${exactDateLabel(entry.createdAt)}. ${entry.title}. Найдено маркеров: ${findingsCount(entry)}. ${analyze(
    entry,
  )
    .map((item) => `${item.title}: ${item.count}`)
    .join(". ")}`;
}
</script>

<template>
  <div class="trend-chart trainer-chart">
    <div class="section-header">
      <strong>Речевые привычки по диктовкам</strong>
      <div class="segmented small" aria-label="Единица графика">
        <button
          :class="{ selected: metric === 'count' }"
          :aria-pressed="metric === 'count'"
          @click="metric = 'count'"
        >
          Количество
        </button>
        <button
          :class="{ selected: metric === 'density' }"
          :aria-pressed="metric === 'density'"
          @click="metric = 'density'"
        >
          На 100 слов
        </button>
      </div>
    </div>
    <p class="muted trainer-chart-caption">
      {{
        metric === "count"
          ? "Число найденных маркеров"
          : "Маркеры на 100 слов исходной расшифровки"
      }}
    </p>
    <svg
      viewBox="0 0 510 194"
      role="group"
      aria-label="Составные столбцы: каждая диктовка показана по категориям. Выберите столбец для разбора."
    >
      <g v-for="tick in scale.ticks" :key="tick">
        <path :d="`M46 ${y(tick)} H486`" class="chart-grid" />
        <text x="36" :y="y(tick) + 4" text-anchor="end" class="chart-axis">
          {{ formatMetric(tick) }}
        </text>
      </g>
      <g
        v-for="column in columns"
        :key="column.entry.id"
        class="chart-column"
        :class="{ selected: selectedId === column.entry.id }"
        role="button"
        tabindex="0"
        :aria-label="description(column.entry)"
        :aria-pressed="selectedId === column.entry.id"
        @click="emit('select', column.entry.id)"
        @keydown.enter.prevent="emit('select', column.entry.id)"
        @keydown.space.prevent="emit('select', column.entry.id)"
      >
        <title>{{ description(column.entry) }}</title>
        <rect
          class="chart-hit-area"
          :x="column.x - barWidth / 2 - 3"
          y="20"
          :width="barWidth + 6"
          height="148"
          rx="3"
        />
        <rect
          v-for="segment in column.segments"
          :key="segment.title"
          :x="column.x - barWidth / 2"
          :y="y(segment.start + segment.value)"
          :width="barWidth"
          :height="(segment.value / scale.max) * 132"
          class="chart-segment"
          :class="categoryClass(segment.title)"
        />
        <path
          v-if="column.value === 0"
          :d="`M${column.x - barWidth / 2} 163 H${column.x + barWidth / 2}`"
          class="chart-empty-bar"
        />
        <text
          v-if="columns.length <= 12"
          :x="column.x"
          :y="y(column.value) - 7"
          text-anchor="middle"
          class="chart-count"
        >
          {{ formatMetric(column.value) }}
        </text>
      </g>
      <text v-if="visible[0]" x="46" y="188" class="chart-axis">
        {{ shortDate(visible[0].createdAt) }}
      </text>
      <text
        v-if="visible.length > 1"
        x="486"
        y="188"
        text-anchor="end"
        class="chart-axis"
      >
        {{ shortDate(visible[visible.length - 1].createdAt) }}
      </text>
    </svg>
    <div class="chart-legend">
      <span v-for="title in findingTitles" :key="title"
        ><i :class="categoryClass(title)" />{{ title }}</span
      >
    </div>
    <small class="muted"
      >{{
        entries.length > visible.length
          ? `Последние ${visible.length} из ${entries.length} диктовок`
          : "От ранних диктовок к последним"
      }}
      · нажмите столбец, чтобы открыть разбор</small
    >
  </div>
</template>

<style scoped>
.trainer-chart {
  min-width: 0;
  gap: var(--fono-space-2);
}
.trainer-chart .section-header {
  flex-wrap: wrap;
  gap: var(--fono-space-2);
  margin-bottom: 0;
}
.trainer-chart-caption {
  font-size: var(--fono-type-xs);
}
.chart-axis,
.chart-count {
  fill: var(--fono-muted);
  font-size: var(--fono-type-xs);
  font-variant-numeric: tabular-nums;
}
.chart-count {
  fill: var(--fono-text);
}
.chart-hit-area {
  fill: transparent;
  stroke: transparent;
}
.chart-column {
  cursor: pointer;
  outline: none;
}
.chart-column:hover .chart-hit-area,
.chart-column:focus-visible .chart-hit-area {
  fill: var(--fono-accent-soft);
  stroke: var(--fono-accent);
}
.chart-column.selected .chart-hit-area {
  stroke: var(--fono-electric-soft);
  stroke-dasharray: 3 3;
}
.chart-segment {
  fill: currentColor;
}
.chart-empty-bar {
  stroke: var(--fono-muted);
  stroke-width: 2;
}
.chart-legend {
  display: flex;
  flex-wrap: wrap;
  gap: var(--fono-space-2) var(--fono-space-3);
  margin-bottom: var(--fono-space-1);
  font-size: var(--fono-type-xs);
  color: var(--fono-muted);
}
.chart-legend span {
  display: inline-flex;
  align-items: center;
  gap: var(--fono-space-1);
}
.chart-legend i {
  width: var(--fono-space-2);
  height: var(--fono-space-2);
  border-radius: var(--fono-radius-sm);
  background: currentColor;
}
.category-0 {
  color: var(--fono-trainer-filler);
}
.category-1 {
  color: var(--fono-trainer-repetition);
}
.category-2 {
  color: var(--fono-trainer-correction);
}
.category-3 {
  color: var(--fono-trainer-unfinished);
}
</style>
