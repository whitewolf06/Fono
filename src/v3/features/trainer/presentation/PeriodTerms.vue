<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { WlButton, WlCard } from "@whitelife-core/ui-kit";
import type { TrainerSnapshot, TrainerTerm } from "../domain/report";
import { formatMetric } from "../application/overview";
const props = defineProps<{ snapshot: TrainerSnapshot; stale: boolean }>();
const emit = defineEmits<{ selectTerm: [term: TrainerTerm] }>();
const showAllTerms = ref(false);
const terms = computed(() =>
  showAllTerms.value ? props.snapshot.terms : props.snapshot.terms.slice(0, 6),
);
const kindLabels = {
  filler: "Слово-паразит",
  repetition: "Повтор",
  self_correction: "Самоисправление",
  unfinished: "Незавершённая фраза",
};
const pluralRules = new Intl.PluralRules("ru-RU");
function countLabel(count: number, one: string, few: string, many: string) {
  const category = pluralRules.select(count);
  return category === "one"
    ? one
    : category === "few" || category === "other"
      ? few
      : many;
}
watch(
  () => props.snapshot.fingerprint,
  () => {
    showAllTerms.value = false;
  },
);
</script>

<template>
  <WlCard
    class="term-ranking trainer-card"
    role="region"
    aria-label="Рейтинг слов и речевых паттернов"
  >
    <div class="section-header">
      <h3>Чаще встречаются</h3>
      <span class="muted ranking-hint">Нажмите, чтобы открыть диктовки</span>
    </div>
    <p v-if="stale" class="notice" role="status">
      Рейтинг и связанные диктовки доступны после обновления отчёта.
    </p>
    <template v-else-if="snapshot.terms.length">
      <div class="term-table-heading" aria-hidden="true">
        <span>Слово или паттерн</span><span>Раз</span><span>На 100 слов</span
        ><span>Диктовок</span>
      </div>
      <div class="term-list">
        <button
          v-for="term in terms"
          :key="term.key"
          class="term-row"
          :aria-label="`${term.label}, ${kindLabels[term.kind]}, ${term.count} ${countLabel(term.count, 'вхождение', 'вхождения', 'вхождений')}, ${formatMetric(term.density)} на 100 слов, ${term.affectedSessions} ${countLabel(term.affectedSessions, 'диктовка', 'диктовки', 'диктовок')}`"
          @click="emit('selectTerm', term)"
        >
          <span class="term-label"
            ><strong>{{ term.label }}</strong
            ><small>{{ kindLabels[term.kind] }}</small></span
          ><strong>{{ term.count }}</strong
          ><span>{{ formatMetric(term.density) }}</span
          ><span>{{ term.affectedSessions }}</span>
        </button>
      </div>
      <WlButton
        v-if="snapshot.terms.length > 6"
        variant="soft"
        size="sm"
        class="ranking-more"
        @click="showAllTerms = !showAllTerms"
        >{{
          showAllTerms
            ? "Показать главные"
            : `Все паттерны · ${snapshot.terms.length}`
        }}</WlButton
      >
    </template>
    <p v-else class="notice">
      {{
        snapshot.includedSessions
          ? "Речевые маркеры в этих диктовках не найдены."
          : "Рейтинг появится после локального разбора записей."
      }}
    </p>
    <p class="muted report-note ranking-note">
      Наблюдения по исходному тексту. Контекст может менять значение маркера.
    </p>
  </WlCard>
</template>

<style scoped>
.trainer-card {
  min-width: 0;
  background: var(--fono-surface);
}
.trainer-card :deep(.wl-card__content) {
  padding: var(--fono-space-4);
}
h3,
h4 {
  margin: 0;
  font-size: var(--fono-type-lg);
}
.term-ranking .section-header {
  display: grid;
  gap: var(--fono-space-2);
  margin-bottom: var(--fono-space-4);
}
.ranking-hint {
  font-size: var(--fono-type-sm);
}
.term-table-heading,
.term-row {
  display: grid;
  grid-template-columns:
    minmax(0, 1fr) var(--fono-trainer-ranking-count-width)
    var(--fono-trainer-ranking-density-width) var(
      --fono-trainer-ranking-sessions-width
    );
  gap: var(--fono-space-2);
  align-items: center;
}
.term-table-heading {
  padding: 0 var(--fono-space-3) var(--fono-space-3);
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
}
.term-table-heading > span:not(:first-child) {
  text-align: right;
  white-space: nowrap;
}
.term-row {
  width: 100%;
  padding: var(--fono-space-3);
  border: 0;
  border-bottom: 1px solid var(--fono-border-soft);
  border-radius: var(--fono-radius-sm);
  background: transparent;
  color: var(--fono-text);
  text-align: left;
  font-size: var(--fono-type-md);
}
.term-row:hover {
  background: var(--fono-accent-soft);
}
.term-row:focus-visible {
  outline: 2px solid var(--fono-accent);
  outline-offset: 2px;
}
.term-row > :not(:first-child) {
  text-align: right;
  font-variant-numeric: tabular-nums;
}
.term-label {
  display: grid;
  gap: var(--fono-space-1);
  min-width: 0;
  overflow-wrap: anywhere;
}
.term-label small {
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
}
.ranking-more {
  margin-top: var(--fono-space-3);
}
.report-note {
  font-size: var(--fono-type-md);
  line-height: 1.7;
}
.ranking-note {
  margin-top: var(--fono-space-4);
}
</style>
