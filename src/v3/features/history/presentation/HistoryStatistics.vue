<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

import { computed } from "vue";
import type { Dictation } from "../../../shared/domain/contracts";
import { dictationFacts, dictationStages } from "../application/statistics";
const props = defineProps<{ entry: Dictation }>();
const facts = computed(() => dictationFacts(props.entry));
const stages = computed(() => dictationStages(props.entry));
</script>

<template>
  <section class="history-statistics" :aria-label="$t('ui.5e15c58f37')">
    <p v-if="entry.metadata?.demo" class="muted">{{ $t("ui.0852a5070f") }}</p>
    <dl>
      <div v-for="fact in facts" :key="fact.title">
        <dt>{{ fact.title }}</dt>
        <dd>{{ fact.value }}</dd>
      </div>
    </dl>
    <details v-if="stages.length" class="history-statistics-stages">
      <summary>{{ $t("ui.4cda89f3d1") }}</summary>
      <dl>
        <div v-for="stage in stages" :key="stage.title">
          <dt>{{ stage.title }}</dt>
          <dd>{{ stage.value }}</dd>
        </div>
      </dl>
    </details>
    <p class="history-statistics-note muted">
      {{ $t("ui.19b9956ba7") }}
      <template v-if="!entry.metadata || entry.metadata.legacy">{{
        $t("ui.600e1887b8")
      }}</template>
    </p>
  </section>
</template>

<style scoped>
.history-statistics {
  padding: var(--fono-space-3) 0;
  border-block: 1px solid var(--fono-border-soft);
}
.history-statistics dl {
  display: grid;
  grid-template-columns: repeat(
    auto-fit,
    minmax(var(--fono-history-stat-width), 1fr)
  );
  gap: var(--fono-space-3);
  margin: 0;
}
.history-statistics dt {
  color: var(--fono-muted);
  font-size: var(--fono-type-xs);
}
.history-statistics dd {
  margin: var(--fono-space-1) 0 0;
  color: var(--fono-secondary);
  font-size: var(--fono-type-sm);
  overflow-wrap: anywhere;
}
.history-statistics-note {
  margin-bottom: 0;
  font-size: var(--fono-type-xs);
}
.history-statistics-stages {
  margin-top: var(--fono-space-3);
}
.history-statistics-stages summary {
  color: var(--fono-secondary);
  font-size: var(--fono-type-sm);
  cursor: pointer;
}
.history-statistics-stages dl {
  margin-top: var(--fono-space-3);
}
</style>
