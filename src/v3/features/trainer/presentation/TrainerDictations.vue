<script setup lang="ts">
import { computed } from "vue";
import { WlButton, WlField, WlSelect } from "@whitelife-core/ui-kit";
import type { Dictation } from "../../../shared/domain/contracts";
import type { TrainerTerm } from "../domain/report";
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
import EmptyState from "../../../shared/presentation/EmptyState.vue";
import TrainerDetails from "./TrainerDetails.vue";
const props = defineProps<{
  entries: Dictation[];
  selectedTerm?: TrainerTerm;
  stale: boolean;
  active: boolean;
}>();
const emit = defineEmits<{ clearTerm: [] }>();
const sort = defineModel<TrainerSort>("sort", { default: "date" });
const sortOptions = [
  { value: "date", label: "Сначала новые" },
  { value: "duration", label: "Сначала долгие" },
  { value: "length", label: "Сначала длинные тексты" },
];
const selectedId = defineModel<string>("selectedId", { default: "" });
const sortedEntries = computed(() =>
  sortTrainerEntries(
    props.selectedTerm
      ? props.entries.filter((entry) =>
          props.selectedTerm!.sessionIds.includes(entry.id),
        )
      : props.entries,
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

function chooseSort(value: unknown) {
  if (value === "date" || value === "duration" || value === "length")
    sort.value = value;
}
</script>

<template>
  <div v-if="selectedTerm && !stale" class="section-header trainer-filter">
    <span
      >Маркер «{{ selectedTerm.label }}» · {{ sortedEntries.length }} из
      {{ entries.length }} диктовок</span
    >
    <WlButton size="sm" variant="soft" @click="emit('clearTerm')"
      >Показать все диктовки</WlButton
    >
  </div>
  <p v-if="stale" class="notice" role="status">
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
              v-if="active"
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

<style scoped>
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
