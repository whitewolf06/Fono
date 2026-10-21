<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed, ref } from "vue";
import {
  WlAccordion,
  WlBadge,
  WlButton,
  WlStatCard,
} from "@whitelife-core/ui-kit";
import type { TrainerReport } from "../domain/report";
import { formatMetric } from "../application/overview";
import {
  reportDate as date,
  reportTime as time,
} from "../application/reportFormat";
const props = defineProps<{
  report: TrainerReport;
  loading: boolean;
  busy: boolean;
  error: string;
  native: boolean;
}>();
const emit = defineEmits<{ refresh: [] }>();
const snapshot = computed(() => props.report.snapshot);
const sourceOpenKeys = ref<string[]>([]);
const sourceItems = [
  {
    key: "trainer-sources",
    get title() {
      return uiMessage("ui.74532ab623");
    },
  },
];
const markerCount = computed(() =>
  Object.values(snapshot.value.counts).reduce((sum, count) => sum + count, 0),
);
const sourceCount = computed(
  () =>
    snapshot.value.includedSessions +
    snapshot.value.excludedSessions +
    snapshot.value.pendingSessions +
    snapshot.value.unavailableSessions,
);
const lastDate = computed(() =>
  date(new Date(Date.parse(snapshot.value.to) - 1).toISOString()),
);
</script>

<template>
  <div class="report-heading">
    <div class="report-context">
      <div class="report-range">
        <h2>{{ date(snapshot.from) }} — {{ lastDate }}</h2>
        <span class="muted">UTC</span>
        <WlBadge
          :value="
            loading
              ? $t('ui.99683bbac5')
              : report.stale
                ? $t('ui.c0aae950c6')
                : error
                  ? $t('ui.04781e915d')
                  : $t('ui.d1afd048fa')
          "
          :variant="report.stale || error ? 'warn' : 'success'"
        />
      </div>
      <div class="report-meta">
        <span
          >{{ $t("ui.1a4e7337d2") }} {{ time(snapshot.generatedAt) }} UTC</span
        >
        <WlBadge
          v-if="!native"
          :value="$t('trainer.demoBadge')"
          variant="gray"
        />
      </div>
    </div>
    <WlButton
      size="sm"
      variant="primary"
      :loading="loading"
      :disabled="busy || (!report.stale && !error)"
      @click="emit('refresh')"
    >
      {{ error && !report.stale ? $t("ui.40f9ba8446") : $t("ui.2f5039b886") }}
    </WlButton>
  </div>
  <p v-if="report.stale" class="notice report-stale" role="status">
    {{ $t("ui.6b1e362896") }}
  </p>
  <p v-if="error" class="error-text" role="alert">{{ error }}</p>
  <div class="report-metrics" :aria-label="$t('ui.8d8e6de88a')">
    <WlStatCard
      :label="$t('ui.d0d47198f3')"
      :value="
        $t('ui.582ebc1444', {
          v0: String(snapshot.includedSessions),
          v1: String(sourceCount),
        })
      "
    />
    <WlStatCard
      :label="$t('ui.bc9f773ccc')"
      :value="formatMetric(snapshot.totalWords)"
    />
    <WlStatCard :label="$t('ui.5a83465bc1')" :value="String(markerCount)" />
    <WlStatCard
      :label="$t('ui.f6da67615a')"
      :value="formatMetric(snapshot.density)"
    />
  </div>
  <WlAccordion
    v-model:open-keys="sourceOpenKeys"
    :items="sourceItems"
    class="report-sources"
  >
    <template #item>
      <div class="source-details">
        <dl class="source-counts">
          <div>
            <dt>{{ $t("ui.96636d8a9c") }}</dt>
            <dd>{{ snapshot.excludedSessions }}</dd>
          </div>
          <div>
            <dt>{{ $t("ui.b22a2454bd") }}</dt>
            <dd>{{ snapshot.pendingSessions }}</dd>
          </div>
          <div>
            <dt>{{ $t("ui.6f37046b14") }}</dt>
            <dd>{{ snapshot.unavailableSessions }}</dd>
          </div>
        </dl>
        <p>{{ $t("ui.00c9fa3cec") }}</p>
        <p class="muted">
          <template v-if="snapshot.historyLimit != null"
            >{{ $t("ui.f1091025af") }} {{ snapshot.historyLimit }}
            {{ $t("ui.f6c00b49f8") }}</template
          >
          <template v-else>{{ $t("ui.60465b65ce") }}</template>
          {{ $t("ui.d2c868a1b9") }} {{ snapshot.retentionDays }}
          {{ $t("ui.2b351fed72")
          }}<template v-if="snapshot.coverageStart">
            {{ $t("ui.f707aae325") }}
            {{ date(snapshot.coverageStart) }}.</template
          >
          {{ $t("ui.bca9fce474") }}
        </p>
        <div class="source-links">
          <RouterLink class="text-link" to="/history">{{
            $t("ui.f1d50bf770")
          }}</RouterLink
          ><RouterLink class="text-link" to="/settings/privacy">{{
            $t("ui.deafab2929")
          }}</RouterLink
          ><small class="muted"
            >{{ $t("ui.606bb27587") }} {{ snapshot.rulesVersion }}</small
          >
        </div>
      </div>
    </template>
  </WlAccordion>
  <p v-if="!snapshot.includedSessions" class="notice report-sample">
    {{ $t("ui.3f87174cce") }}
  </p>
  <p v-else-if="snapshot.insufficientData" class="notice report-sample">
    {{ $t("ui.2246e5ba1e") }}
  </p>
</template>

<style scoped>
.report-heading,
.report-range,
.report-meta,
.source-links {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--fono-space-2) var(--fono-space-3);
}
.report-heading {
  justify-content: space-between;
  align-items: flex-start;
  gap: var(--fono-space-4);
  margin-bottom: var(--fono-space-4);
}
.report-context {
  display: grid;
  gap: var(--fono-space-2);
}
.report-range h2 {
  margin: 0;
  font-size: var(--fono-type-xl);
}
.report-range > span {
  font-size: var(--fono-type-sm);
}
.report-meta {
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
}
.report-stale,
.report-sample {
  margin: 0 0 var(--fono-space-4);
  font-size: var(--fono-type-md);
}
.error-text {
  margin-bottom: var(--fono-space-4);
}
.report-metrics {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: var(--fono-space-3);
}
.report-metrics :deep(.wl-card) {
  min-width: 0;
  background: var(--fono-surface);
}
.report-metrics :deep(.wl-card__content) {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--fono-space-2);
  padding: var(--fono-space-2) var(--fono-space-3);
}
.report-metrics :deep(.wl-stat-card__label) {
  text-transform: none;
  letter-spacing: normal;
  font-size: var(--fono-type-sm);
}
.report-metrics :deep(.wl-stat-card__value) {
  margin-top: 0;
  font-size: var(--fono-type-xl);
  line-height: 1.5;
  white-space: nowrap;
}
.report-sources {
  max-width: none;
  margin-block: var(--fono-space-4);
}
.source-details {
  display: grid;
  gap: var(--fono-space-4);
  font-size: var(--fono-type-md);
  line-height: 1.7;
}
.source-counts,
.priority-details {
  margin: 0;
}
.source-counts {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: var(--fono-space-3);
}
.source-counts > div {
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  align-items: center;
  gap: var(--fono-space-2);
  padding: var(--fono-space-2) var(--fono-space-3);
  background: var(--fono-surface-raised);
  border-radius: var(--fono-radius-sm);
}
.source-counts dt {
  color: var(--fono-muted);
}
.source-counts dd {
  margin: 0;
  color: var(--fono-text);
  font-weight: 600;
}
.source-links {
  gap: var(--fono-space-3) var(--fono-space-5);
}
.source-links small {
  font-size: var(--fono-type-sm);
}

@media (max-width: 780px) {
  .report-metrics {
    gap: var(--fono-space-2);
  }
  .report-metrics :deep(.wl-card__content) {
    display: grid;
    align-content: start;
    justify-content: start;
    gap: var(--fono-space-1);
    padding: var(--fono-space-2) var(--fono-space-3);
  }
  .report-metrics :deep(.wl-stat-card__label) {
    font-size: var(--fono-type-sm);
  }
}
</style>
