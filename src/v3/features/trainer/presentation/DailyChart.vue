<script setup lang="ts">
import { computed, ref } from "vue";
import type { TrainerDay } from "../domain/report";
import { chartScale, formatMetric } from "../application/overview";

const props = defineProps<{ daily: TrainerDay[] }>();
const metric = ref<"count" | "density">("count");
const categories = [
  { key: "filler", title: "Слова-паразиты" },
  { key: "repetition", title: "Повторы" },
  { key: "selfCorrection", title: "Самоисправления" },
  { key: "unfinished", title: "Незавершённые фразы" },
] as const;
const columns = computed(() =>
  props.daily.map((day, index) => {
    let total = 0;
    const segments = categories.map((category) => {
      const count = day.counts[category.key];
      const value =
        metric.value === "density"
          ? day.words > 0
            ? (count * 100) / day.words
            : 0
          : count;
      const segment = { ...category, value, start: total };
      total += value;
      return segment;
    });
    return {
      day,
      segments,
      total,
      x: 52 + (index + 0.5) * (552 / Math.max(1, props.daily.length)),
    };
  }),
);
const scale = computed(() => chartScale(columns.value.map((day) => day.total)));
const width = computed(() =>
  Math.min(38, (552 / Math.max(1, props.daily.length)) * 0.7),
);
const y = (value: number) => 164 - (value / scale.value.max) * 132;
const dateLabel = (date: string) =>
  new Date(date).toLocaleDateString("ru-RU", {
    day: "numeric",
    month: "short",
    timeZone: "UTC",
  });
function description(day: TrainerDay): string {
  const total =
    day.counts.filler +
    day.counts.repetition +
    day.counts.selfCorrection +
    day.counts.unfinished;
  return (
    dateLabel(day.date) +
    ": диктовок " +
    day.sessions +
    ", слов " +
    day.words +
    ", маркеров " +
    total +
    "."
  );
}
</script>

<template>
  <section class="period-daily-chart">
    <div class="section-header daily-toolbar">
      <h3>По дням</h3>
      <div class="segmented small" aria-label="Единица дневного графика">
        <button
          :aria-pressed="metric === 'count'"
          :class="{ selected: metric === 'count' }"
          @click="metric = 'count'"
        >
          Количество
        </button>
        <button
          :aria-pressed="metric === 'density'"
          :class="{ selected: metric === 'density' }"
          @click="metric = 'density'"
        >
          На 100 слов
        </button>
      </div>
    </div>
    <p class="muted daily-caption">
      {{
        metric === "density"
          ? "Плотность маркеров в исходных расшифровках каждого дня."
          : "Число найденных маркеров в исходных расшифровках каждого дня."
      }}
      Пустые дни входят в выбранный период.
    </p>
    <svg
      viewBox="0 0 640 202"
      role="img"
      aria-label="Дневной график речевых маркеров по четырём категориям"
    >
      <g v-for="tick in scale.ticks" :key="tick">
        <path :d="'M52 ' + y(tick) + ' H604'" class="daily-grid" />
        <text x="42" :y="y(tick) + 4" text-anchor="end" class="daily-axis">
          {{ formatMetric(tick) }}
        </text>
      </g>
      <g v-for="column in columns" :key="column.day.date">
        <title>{{ description(column.day) }}</title>
        <rect
          v-for="segment in column.segments"
          :key="segment.key"
          :x="column.x - width / 2"
          :y="y(segment.start + segment.value)"
          :width="width"
          :height="(segment.value / scale.max) * 132"
          :class="'daily-' + segment.key"
        />
        <path
          v-if="column.total === 0"
          :d="'M' + (column.x - width / 2) + ' 163 H' + (column.x + width / 2)"
          class="daily-zero"
        />
      </g>
      <text v-if="daily[0]" x="52" y="190" class="daily-axis">
        {{ dateLabel(daily[0].date) }}
      </text>
      <text
        v-if="daily.length > 1"
        x="604"
        y="190"
        text-anchor="end"
        class="daily-axis"
      >
        {{ dateLabel(daily[daily.length - 1].date) }}
      </text>
    </svg>
    <div class="daily-legend">
      <span v-for="category in categories" :key="category.key"
        ><i :class="'daily-' + category.key" />{{ category.title }}</span
      >
    </div>
  </section>
</template>

<style scoped>
.daily-toolbar {
  flex-wrap: wrap;
  gap: var(--fono-space-3);
}
.daily-toolbar h3 {
  margin: 0;
  font-size: var(--fono-type-lg);
}
.daily-caption {
  font-size: var(--fono-type-sm);
}
svg {
  display: block;
  width: 100%;
}
.daily-grid {
  fill: none;
  stroke: var(--fono-border-soft);
}
.daily-axis {
  fill: var(--fono-muted);
  font-size: var(--fono-type-xs);
}
.daily-zero {
  stroke: var(--fono-border);
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
.daily-legend {
  display: flex;
  flex-wrap: wrap;
  gap: var(--fono-space-2) var(--fono-space-4);
  color: var(--fono-muted);
  font-size: var(--fono-type-xs);
}
.daily-legend span {
  display: inline-flex;
  align-items: center;
  gap: var(--fono-space-1);
}
.daily-legend i {
  width: var(--fono-space-2);
  height: var(--fono-space-2);
  border-radius: var(--fono-radius-sm);
}
</style>
