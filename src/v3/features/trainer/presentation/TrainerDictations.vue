<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

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
  {
    value: "date",
    get label() {
      return uiMessage("ui.0cd8808fe1");
    },
  },
  {
    value: "duration",
    get label() {
      return uiMessage("ui.453635acbb");
    },
  },
  {
    value: "length",
    get label() {
      return uiMessage("ui.c4817ab556");
    },
  },
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
      >{{ $t("ui.f470cbf9be") }}{{ selectedTerm.label }}» ·
      {{ sortedEntries.length }} {{ $t("ui.7f4adf318e") }} {{ entries.length }}
      {{ $t("ui.5b0b15f4e5") }}</span
    >
    <WlButton size="sm" variant="soft" @click="emit('clearTerm')">{{
      $t("ui.5a2c8b6ecf")
    }}</WlButton>
  </div>
  <p v-if="stale" class="notice" role="status">{{ $t("ui.3cabb6fe6e") }}</p>
  <template v-else-if="sortedEntries.length">
    <div class="master-detail trainer-master-detail">
      <div class="entry-list trainer-entry-list">
        <div class="trainer-list-toolbar">
          <h3>{{ $t("ui.8eaba588f4") }}</h3>
          <WlField v-slot="field" :label="$t('ui.66d6d374ce')">
            <WlSelect
              v-if="active"
              v-bind="field"
              :model-value="sort"
              :options="sortOptions"
              option-label="label"
              option-value="value"
              size="md"
              :aria-label="$t('ui.66d6d374ce')"
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
            ><span>{{ wordCount(entry) }} {{ $t("ui.1aed0436db") }}</span
            ><span
              >{{ findingsCount(entry) }} {{ $t("ui.59722c3c39") }}</span
            ></span
          >
        </button>
      </div>
      <TrainerDetails v-if="selected" :entry="selected" />
    </div>
  </template>
  <EmptyState
    v-else
    :title="selectedTerm ? $t('ui.de574ce325') : $t('ui.30df585fff')"
    :text="selectedTerm ? $t('ui.035bb30c4b') : $t('ui.2b147e02c6')"
    ><RouterLink to="/history" class="text-link">{{
      $t("ui.f1d50bf770")
    }}</RouterLink></EmptyState
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
