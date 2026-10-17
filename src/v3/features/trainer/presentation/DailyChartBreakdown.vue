<script setup lang="ts">
import { computed } from "vue";
import { WlField, WlSelect } from "@whitelife-core/ui-kit";
import type { TrainerDay } from "../domain/report";
import { formatMetric, type ChartMetric } from "../application/overview";
import { dailyChartColumns } from "../application/chart";
import { reportDate as dateLabel } from "../application/reportFormat";
const props = defineProps<{
  daily: TrainerDay[];
  selected?: ReturnType<typeof dailyChartColumns>[number];
  metric: ChartMetric;
  active: boolean;
}>();
const emit = defineEmits<{ select: [date: string] }>();
const dateOptions = computed(() =>
  props.daily.map((day) => ({ label: dateLabel(day.date), value: day.date })),
);
function selectDate(value: unknown) {
  if (typeof value === "string") emit("select", value);
}
</script>

<template>
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
    <dl class="daily-legend" aria-label="Маркеры выбранного дня по категориям">
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
</template>

<style scoped>
.daily-caption {
  margin: 0;
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
  line-height: 1.5;
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
