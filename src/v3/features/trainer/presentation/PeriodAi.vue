<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed, ref, watch } from "vue";
import { WlBadge, WlButton, WlCard, WlCheckbox } from "@whitelife-core/ui-kit";
import type { TrainerReport } from "../domain/report";
import {
  periodAiDisabledReason,
  periodConsentKey,
} from "../application/periodReportController";
import { reportTime as time } from "../application/reportFormat";
import AiContextSummary from "./AiContextSummary.vue";
const props = defineProps<{
  report: TrainerReport;
  busy: boolean;
  analyzing: boolean;
  cancelling: boolean;
  connectionKey?: string;
}>();
const emit = defineEmits<{ analyze: [shareText: boolean]; cancel: [] }>();
const shareText = ref(false);
const configuration = computed(() => props.report.aiConfiguration);
const aiContext = computed(() =>
  shareText.value
    ? props.report.aiContext?.configuredShared
    : props.report.aiContext?.metricsOnly,
);
const blocked = computed(() =>
  periodAiDisabledReason(props.report, shareText.value),
);
const scopeLabels = {
  get metrics_only() {
    return uiMessage("ui.6935b172a3");
  },
  get findings() {
    return uiMessage("ui.a99905f9c8");
  },
  get original_text() {
    return uiMessage("trainer.scopeOriginal");
  },
};
watch(
  () => periodConsentKey(props.report, props.connectionKey),
  () => {
    shareText.value = false;
  },
);
</script>

<template>
  <section class="period-ai" :aria-label="$t('ui.a40a4562d4')">
    <WlCard class="ai-controls trainer-card">
      <div class="section-header ai-heading">
        <div>
          <h3>{{ $t("ui.b881eff258") }}</h3>
          <p class="selected-model">
            {{ configuration.model || $t("ui.30bd8ad2b6") }}
          </p>
        </div>
        <RouterLink
          class="text-link"
          to="/settings/privacy?field=trainerModel"
          >{{ $t("ui.de885e7fab") }}</RouterLink
        >
      </div>
      <p class="report-connection">
        <span class="muted">{{ $t("ui.ab71afb97f") }}</span>
        <strong>{{ configuration.profileName || $t("ui.2afc8921e4") }}</strong>
      </p>
      <div
        v-if="configuration.dataScope !== 'metrics_only'"
        class="report-sharing"
      >
        <WlCheckbox v-model="shareText" :disabled="busy">{{
          configuration.dataScope === "original_text"
            ? $t("ui.4f1810d07b")
            : $t("ui.d55e6b59b9")
        }}</WlCheckbox>
        <p class="muted report-note">
          {{
            configuration.dataScope === "original_text"
              ? $t("ui.ebfde0a779")
              : $t("ui.bf633d0d30")
          }}
        </p>
      </div>
      <p v-if="configuration.cloud" class="notice report-cloud">
        {{ $t("ui.de6b778ddd") }}
        {{ shareText ? $t("ui.305347b822") : $t("ui.6935b172a3") }}
        {{ $t("ui.9ad35552e8")
        }}<template v-if="!configuration.cloudConsent">
          {{ $t("ui.16f87a1a74") }}</template
        >
      </p>
      <AiContextSummary v-if="aiContext" :context="aiContext" />
      <div class="actions report-actions">
        <WlButton
          size="sm"
          variant="primary"
          :loading="analyzing"
          :disabled="busy || !!blocked"
          @click="emit('analyze', shareText)"
          >{{ $t("ui.b010e98c9c") }}</WlButton
        ><WlButton
          v-if="analyzing || cancelling"
          size="sm"
          variant="soft"
          :loading="cancelling"
          :disabled="cancelling"
          @click="emit('cancel')"
          >{{ $t("ui.d81229e511") }}</WlButton
        >
        <p class="muted report-launch-note">{{ $t("ui.e5c09ae5b6") }}</p>
      </div>
      <p v-if="blocked" class="muted report-note" role="status">
        {{ blocked }}
      </p>
    </WlCard>
    <WlCard
      v-if="report.aiSummary"
      class="period-ai-result trainer-card"
      role="region"
      :aria-label="$t('ui.79041ca6ef')"
    >
      <p v-if="report.aiStale" class="notice">{{ $t("ui.507795031c") }}</p>
      <div class="saved-ai-heading">
        <h4>{{ $t("ui.e7493c4455") }}</h4>
        <small class="muted"
          >{{ report.aiSummary.profileName }} · {{ report.aiSummary.model }} ·
          {{ time(report.aiSummary.generatedAt) }} UTC</small
        >
      </div>
      <p class="summary-text">{{ report.aiSummary.summary }}</p>
      <p class="muted report-note">
        {{ $t("ui.3591af7ac5") }} {{ scopeLabels[report.aiSummary.dataScope] }}.
      </p>
      <AiContextSummary
        v-if="report.aiSummary.context"
        :context="report.aiSummary.context"
        saved
      />
      <ol class="period-priorities">
        <li
          v-for="(priority, index) in report.aiSummary.priorities"
          :key="index"
        >
          <div class="priority-heading">
            <WlBadge :value="index + 1" variant="accent" /><strong>{{
              priority.title
            }}</strong>
          </div>
          <dl class="priority-details">
            <div>
              <dt>{{ $t("ui.0369683b41") }}</dt>
              <dd>{{ priority.basis }}</dd>
            </div>
            <div>
              <dt>{{ $t("ui.64dd4b27bb") }}</dt>
              <dd>{{ priority.exercise }}</dd>
            </div>
          </dl>
        </li>
      </ol>
    </WlCard>
  </section>
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
.report-note {
  font-size: var(--fono-type-md);
  line-height: 1.7;
}
.priority-details {
  margin: 0;
}
.period-ai {
  display: grid;
  gap: var(--fono-space-5);
}
.period-ai-result :deep(.wl-card__content) {
  display: grid;
  gap: var(--fono-space-4);
}
.ai-controls :deep(.wl-card__content) {
  display: grid;
  gap: var(--fono-space-3);
}
.ai-heading {
  margin-bottom: 0;
  flex-wrap: wrap;
  gap: var(--fono-space-3);
  align-items: flex-start;
}
.ai-heading > div {
  display: grid;
  gap: var(--fono-space-1);
}
.ai-heading h3 {
  font-size: var(--fono-type-md);
  font-weight: 400;
  color: var(--fono-muted);
}
.ai-heading .selected-model {
  font-size: var(--fono-type-xl);
  font-weight: 600;
  color: var(--fono-text);
  overflow-wrap: anywhere;
}
.report-connection {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: var(--fono-space-2);
  font-size: var(--fono-type-sm);
}
.report-connection strong {
  font-weight: 500;
  overflow-wrap: anywhere;
}
.report-sharing {
  display: grid;
  gap: var(--fono-space-3);
  padding: var(--fono-space-3);
  border: 1px solid var(--fono-border);
  border-radius: var(--fono-radius);
}
.report-actions {
  flex-wrap: wrap;
  gap: var(--fono-space-3);
}
.report-launch-note {
  flex: 1 1 var(--fono-trainer-day-select-width);
  font-size: var(--fono-type-sm);
  line-height: 1.6;
}
.saved-ai-heading {
  display: grid;
  gap: var(--fono-space-2);
}
.saved-ai-heading small {
  font-size: var(--fono-type-sm);
  line-height: 1.6;
  overflow-wrap: anywhere;
}
.summary-text,
.priority-details dd {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  font-size: var(--fono-type-md);
  line-height: 1.7;
}
.period-priorities {
  display: grid;
  gap: var(--fono-space-4);
  padding: 0;
  margin: 0;
  list-style: none;
}
.period-priorities li {
  display: grid;
  gap: var(--fono-space-4);
  border-top: 1px solid var(--fono-border);
  padding-top: var(--fono-space-4);
}
.priority-heading {
  display: flex;
  align-items: center;
  gap: var(--fono-space-3);
  font-size: var(--fono-type-lg);
}
.priority-details {
  display: grid;
  gap: var(--fono-space-3);
}
.priority-details > div {
  display: grid;
  gap: var(--fono-space-1);
}
.priority-details dt {
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
}
.priority-details dd {
  margin: 0;
  color: var(--fono-text);
}
</style>
