<script setup lang="ts">
import { computed } from "vue";
import { message as $t } from "../../../shared/domain/messages";
import type { TrainerSpeechTiming } from "../domain/report";
import { speechTimingMetrics } from "../domain/speechTiming";
import { formatMetric } from "../application/overview";
const props = defineProps<{
  timing?: TrainerSpeechTiming | null;
  sessions?: number;
  compact?: boolean;
}>();
const metrics = computed(() => speechTimingMetrics(props.timing));
const items = computed(() => [
  {
    label: $t("speechTiming.rate"),
    value: metrics.value.wordsPerMinute,
    unit: $t("speechTiming.wpm"),
  },
  {
    label: $t("speechTiming.share"),
    value: metrics.value.pausePercent,
    unit: "%",
  },
  {
    label: $t("speechTiming.average"),
    value:
      metrics.value.averagePauseMs == null
        ? null
        : metrics.value.averagePauseMs / 1000,
    unit: $t("speechTiming.seconds"),
  },
  {
    label: $t("speechTiming.maximum"),
    value:
      metrics.value.maxPauseMs == null ? null : metrics.value.maxPauseMs / 1000,
    unit: $t("speechTiming.seconds"),
  },
]);
</script>
<template>
  <section
    class="speech-timing"
    :class="{ compact }"
    :aria-label="$t('speechTiming.title')"
  >
    <h3 v-if="!compact">{{ $t("speechTiming.title") }}</h3>
    <dl>
      <div v-for="item in items" :key="item.label">
        <dt>{{ item.label }}</dt>
        <dd>
          {{
            item.value == null
              ? $t("speechTiming.unknown")
              : `${formatMetric(item.value)} ${item.unit}`
          }}
        </dd>
      </div>
    </dl>
    <p>
      {{
        $t("speechTiming.coverage", {
          measured: String(timing?.measuredSessions ?? 0),
          total: String(
            (timing?.measuredSessions ?? 0) +
              (timing?.unknownSessions ?? sessions ?? 0),
          ),
          unknown: String(timing?.unknownSessions ?? sessions ?? 0),
        })
      }}
    </p>
    <p v-if="!compact">{{ $t("speechTiming.definition") }}</p>
  </section>
</template>
<style scoped>
.speech-timing {
  display: grid;
  gap: var(--fono-space-2);
  margin-block: var(--fono-space-4);
  min-width: 0;
}
h3 {
  margin: 0;
  font-size: var(--fono-type-lg);
}
dl {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: var(--fono-space-3);
  margin: 0;
}
dl > div {
  min-width: 0;
  padding: var(--fono-space-3);
  background: var(--fono-surface-raised);
  border-radius: var(--fono-radius-sm);
}
dt,
p {
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
  line-height: 1.5;
}
dd {
  margin: var(--fono-space-2) 0 0;
  font-size: var(--fono-type-lg);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
p {
  margin: 0;
}
.compact {
  margin-block: var(--fono-space-2);
}
@media (max-width: 780px) {
  dl {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
