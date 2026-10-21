<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed, ref, watch } from "vue";
import { WlButton, WlCard } from "@whitelife-core/ui-kit";
import type { TrainerSnapshot, TrainerTerm } from "../domain/report";
import { formatMetric } from "../application/overview";
import { uiIntlLocale } from "../../../shared/domain/locale";
const props = defineProps<{ snapshot: TrainerSnapshot; stale: boolean }>();
const emit = defineEmits<{ selectTerm: [term: TrainerTerm] }>();
const showAllTerms = ref(false);
const terms = computed(() =>
  showAllTerms.value ? props.snapshot.terms : props.snapshot.terms.slice(0, 6),
);
const kindLabels = {
  get filler() {
    return uiMessage("ui.a894b25009");
  },
  get repetition() {
    return uiMessage("ui.4096048ce1");
  },
  get self_correction() {
    return uiMessage("ui.c324a61d6c");
  },
  get unfinished() {
    return uiMessage("ui.8e97136593");
  },
};
const pluralRules = computed(() => new Intl.PluralRules(uiIntlLocale()));
function countLabel(count: number, one: string, few: string, many: string) {
  const category = pluralRules.value.select(count);
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
    :aria-label="$t('ui.89ca82ed90')"
  >
    <div class="section-header">
      <h3>{{ $t("ui.0fbe9f755a") }}</h3>
      <span class="muted ranking-hint">{{ $t("ui.fd755396fa") }}</span>
    </div>
    <p v-if="stale" class="notice" role="status">{{ $t("ui.fc41e34a35") }}</p>
    <template v-else-if="snapshot.terms.length">
      <div class="term-table-heading" aria-hidden="true">
        <span>{{ $t("ui.a59f33874a") }}</span
        ><span>{{ $t("ui.ea3be88a83") }}</span
        ><span>{{ $t("ui.f6da67615a") }}</span
        ><span>{{ $t("ui.d0d47198f3") }}</span>
      </div>
      <div class="term-list">
        <button
          v-for="term in terms"
          :key="term.key"
          class="term-row"
          :aria-label="
            $t('ui.e275b383a9', {
              v0: String(term.label),
              v1: String(kindLabels[term.kind]),
              v2: String(term.count),
              v3: String(
                countLabel(
                  term.count,
                  $t('ui.9c7829e2d5'),
                  $t('ui.9966ab1a4a'),
                  $t('ui.b9dfa7e6b3'),
                ),
              ),
              v4: String(formatMetric(term.density)),
              v5: String(term.affectedSessions),
              v6: String(
                countLabel(
                  term.affectedSessions,
                  $t('ui.6c0334829b'),
                  $t('ui.682e643dad'),
                  $t('ui.5b0b15f4e5'),
                ),
              ),
            })
          "
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
            ? $t("ui.425b42b259")
            : $t("ui.7a5b2d7224", { v0: String(snapshot.terms.length) })
        }}</WlButton
      >
    </template>
    <p v-else class="notice">
      {{
        snapshot.includedSessions ? $t("ui.8761d9414b") : $t("ui.ad7f42f44f")
      }}
    </p>
    <p class="muted report-note ranking-note">{{ $t("ui.5ac7b5358a") }}</p>
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
