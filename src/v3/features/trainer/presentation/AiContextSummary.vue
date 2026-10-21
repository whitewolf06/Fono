<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

import { WlBadge } from "@whitelife-core/ui-kit";
import type { TrainerAiContext } from "../domain/report";
import { formatMetric } from "../application/overview";

defineProps<{ context: TrainerAiContext; saved?: boolean }>();
const size = (bytes: number) => formatMetric(bytes / 1024);
</script>

<template>
  <div
    class="ai-context"
    :aria-label="saved ? $t('ui.a56c903c8b') : $t('ui.36cad868c5')"
  >
    <div class="context-heading">
      <strong>{{ saved ? $t("ui.a56c903c8b") : $t("ui.f4d567d67f") }}</strong>
      <WlBadge
        :value="context.reduced ? $t('ui.dcb9808283') : $t('ui.c0064ee5cc')"
        :variant="context.reduced ? 'warn' : 'gray'"
      />
    </div>
    <dl class="context-counts">
      <div>
        <dt>{{ $t("ui.142ebed7e2") }}</dt>
        <dd>{{ context.metricsSessions }}</dd>
      </div>
      <div>
        <dt>{{ $t("ui.01830bf551") }}</dt>
        <dd>{{ formatMetric(context.metricsWords) }}</dd>
      </div>
      <div>
        <dt>{{ $t("ui.783e6db32b") }}</dt>
        <dd>
          {{ size(context.payloadBytes) }} /
          {{ size(context.payloadByteLimit) }} {{ $t("ui.4a0e8a9bfa") }}
        </dd>
      </div>
      <div>
        <dt>{{ $t("ui.0b1b2f5584") }}</dt>
        <dd>
          {{ context.patternsShared }} {{ $t("ui.7f4adf318e") }}
          {{ context.patternsAvailable }}
        </dd>
      </div>
      <template v-if="context.dataScope === 'original_text'">
        <div>
          <dt>{{ $t("ui.d624c7f988") }}</dt>
          <dd>
            {{ context.sharedSessions }} {{ $t("ui.7f4adf318e") }}
            {{ context.availableSessions }}
          </dd>
        </div>
        <div>
          <dt>{{ $t("ui.7a3e0b7611") }}</dt>
          <dd>
            {{ formatMetric(context.sharedWords) }} {{ $t("ui.7f4adf318e") }}
            {{ formatMetric(context.availableWords) }}
          </dd>
        </div>
        <div>
          <dt>{{ $t("ui.165afb148a") }}</dt>
          <dd>{{ context.shortenedSessions }}</dd>
        </div>
      </template>
      <template v-if="context.dataScope !== 'metrics_only'">
        <div>
          <dt>{{ $t("ui.f97d85216e") }}</dt>
          <dd>
            {{ context.examplesShared }} {{ $t("ui.7f4adf318e") }}
            {{ context.examplesAvailable }}
          </dd>
        </div>
      </template>
      <div v-if="context.dataScope === 'findings'">
        <dt>{{ $t("ui.120516a808") }}</dt>
        <dd>
          {{ formatMetric(context.sharedWords) }} {{ $t("ui.7f4adf318e") }}
          {{ formatMetric(context.availableWords) }}
        </dd>
      </div>
    </dl>
    <p v-if="context.omittedCharacters" class="context-omitted">
      {{
        context.dataScope === "original_text"
          ? $t("ui.8cf76031fb")
          : $t("ui.c448ec6000")
      }}
      {{ $t("ui.bfb327e442") }} {{ formatMetric(context.omittedWords) }}
      {{ $t("ui.51e8ff21a3") }} {{ formatMetric(context.omittedCharacters) }}
      {{ $t("ui.7de4651234") }}
      <template v-if="context.omittedSessions"
        >{{ $t("ui.d4e065acc1") }} {{ context.omittedSessions }}.</template
      >
    </p>
    <p v-if="!saved" class="context-note">
      {{ $t("ui.b408b00971") }} {{ size(context.payloadByteLimit) }}
      {{ $t("ui.85ac18c982") }}
      <template v-if="context.dataScope === 'original_text'"
        >{{ $t("ui.4a38273155") }} {{ context.limits.maxSharedSessions }}
        {{ $t("ui.1122a1c32f") }}
        {{ formatMetric(context.limits.maxCharactersPerSession) }}
        {{ $t("ui.e87ad0ed53") }}</template
      >
      <template v-if="context.reduced">{{ $t("ui.9e63f2a8e8") }}</template>
    </p>
  </div>
</template>

<style scoped>
.ai-context {
  display: grid;
  gap: var(--fono-space-3);
  min-width: 0;
  padding: var(--fono-space-3);
  border: 1px solid var(--fono-border);
  border-radius: var(--fono-radius);
  font-size: var(--fono-type-sm);
  line-height: 1.6;
}
.context-heading {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--fono-space-2);
}
.context-counts {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: var(--fono-space-2) var(--fono-space-4);
  margin: 0;
}
.context-counts > div {
  display: grid;
  gap: var(--fono-space-1);
}
.context-counts dt,
.context-note {
  color: var(--fono-muted);
}
.context-counts dd {
  margin: 0;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  overflow-wrap: anywhere;
}
.context-omitted {
  color: var(--fono-warning);
}
.context-note,
.context-omitted {
  margin: 0;
  overflow-wrap: anywhere;
}
@media (max-width: 780px) {
  .context-counts {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
