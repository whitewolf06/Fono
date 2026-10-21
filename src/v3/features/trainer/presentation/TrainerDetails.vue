<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";
import { translateAuthored } from "../../../shared/domain/messages";

import { computed, watch } from "vue";
import { WlButton, WlCard } from "@whitelife-core/ui-kit";
import type { Dictation } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import { engineNames } from "../../../shared/domain/speechModels";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import { analyze } from "../domain/analysis";
import { chartCategories } from "../application/chart";
import {
  transcriptHighlights,
  type TranscriptHighlight,
} from "../application/transcriptHighlights";
import {
  exactDateLabel,
  findingsCount,
  formatMetric,
  recordingDurationLabel,
  wordCount,
} from "../application/overview";

const props = defineProps<{ entry: Dictation }>();
const workspace = useWorkspace();
const { busy, run, error } = useFeedback();
const findings = computed(() => analyze(props.entry));
const categoryCounts = computed(() =>
  chartCategories.map((category) => ({
    ...category,
    count:
      findings.value.find((finding) => finding.title === category.title)
        ?.count ?? 0,
  })),
);
const highlightedTranscript = computed(() =>
  transcriptHighlights(props.entry.original || "", findings.value),
);
const density = computed(() =>
  wordCount(props.entry) > 0
    ? (findingsCount(props.entry) * 100) / wordCount(props.entry)
    : 0,
);
const technicalFacts = computed(() => {
  const metadata = props.entry.metadata;
  const facts: string[] = [];
  if (metadata?.recognizer) facts.push(engineNames[metadata.recognizer]);
  if (metadata?.backend)
    facts.push(
      { cpu: "CPU", cuda: "NVIDIA CUDA", vulkan: "Vulkan" }[metadata.backend],
    );
  if (metadata?.model) facts.push(metadata.model);
  if (
    metadata?.generationDurationMs != null &&
    Number.isFinite(metadata.generationDurationMs) &&
    metadata.generationDurationMs >= 0
  )
    facts.push(
      uiMessage("ui.60527957f2", {
        v0: String(formatMetric(metadata.generationDurationMs / 1000)),
      }),
    );
  return facts.join(" · ");
});
watch(
  () => props.entry.id,
  () => {
    error.value = "";
  },
);
function markerTitle(segment: TranscriptHighlight): string {
  const title =
    chartCategories.find((category) => category.key === segment.category)
      ?.title || "";
  const markers = segment.markers
    ?.map((marker) => `№${marker.number}: ${translateAuthored(marker.title)}`)
    .join("; ");
  return markers
    ? `${translateAuthored(title)}. ${markers}`
    : translateAuthored(title);
}
</script>

<template>
  <article class="detail-pane trainer-detail">
    <header class="trainer-detail-heading">
      <h2>{{ entry.title }}</h2>
      <time :datetime="entry.createdAt" :title="entry.createdAt">{{
        exactDateLabel(entry.createdAt)
      }}</time>
    </header>
    <dl class="trainer-detail-facts">
      <div>
        <dt><AppIcon name="clock" :size="14" />{{ $t("ui.2a32607189") }}</dt>
        <dd>{{ recordingDurationLabel(entry) }}</dd>
      </div>
      <div>
        <dt>{{ $t("ui.bc9f773ccc") }}</dt>
        <dd>{{ wordCount(entry) }}</dd>
      </div>
      <div>
        <dt>{{ $t("ui.5a83465bc1") }}</dt>
        <dd>
          {{ findingsCount(entry) }}
          <small>· {{ formatMetric(density) }} {{ $t("ui.03c04ee5b8") }}</small>
        </dd>
      </div>
    </dl>
    <p v-if="technicalFacts" class="muted trainer-technical">
      {{ technicalFacts }}
    </p>
    <WlCard class="trainer-detail-card trainer-original">
      <template #title>
        <div class="trainer-original-heading">
          <h3>{{ $t("ui.421252fcb5") }}</h3>
          <WlButton
            size="sm"
            variant="soft"
            :disabled="busy"
            @click="
              run(
                () => workspace.copy(entry.original || ''),
                $t('ui.26618376ef'),
              )
            "
          >
            <template #icon><AppIcon name="copy" :size="14" /></template
            >{{ $t("ui.1ecddf40bf") }}
          </WlButton>
        </div>
      </template>
      <div
        class="trainer-original-text"
        role="group"
        :aria-label="$t('ui.456b6e72c8')"
        :title="$t('ui.111dfcd2c1')"
        tabindex="0"
      >
        <template
          v-for="(segment, index) in highlightedTranscript"
          :key="index"
        >
          <mark
            v-if="segment.highlighted"
            class="transcript-marker"
            :data-category="segment.category"
            :title="markerTitle(segment)"
            >{{ segment.text }}</mark
          >
          <span v-else>{{ segment.text }}</span>
          <sup
            v-for="marker in segment.markers"
            :key="marker.number"
            class="transcript-marker-number"
            :data-category="marker.category"
            :data-number="marker.number"
            :title="`№${marker.number}: ${translateAuthored(marker.title)}`"
            aria-hidden="true"
          />
        </template>
      </div>
      <dl class="trainer-category-counts" :aria-label="$t('ui.ce223b5d54')">
        <div
          v-for="category in categoryCounts"
          :key="category.key"
          :data-category="category.key"
        >
          <dt>
            <i
              class="trainer-category-dot"
              :data-category="category.key"
              aria-hidden="true"
            /><span>{{ translateAuthored(category.title) }}</span>
          </dt>
          <dd>{{ category.count }}</dd>
        </div>
      </dl>
    </WlCard>
    <p v-if="error" role="alert" class="error-text">{{ error }}</p>
  </article>
</template>

<style scoped>
.trainer-detail {
  min-width: 0;
  display: grid;
  gap: var(--fono-space-5);
  align-content: start;
}
.trainer-detail-heading {
  display: grid;
  gap: var(--fono-space-2);
}
.trainer-detail-heading h2 {
  overflow-wrap: anywhere;
  font-size: var(--fono-type-xl);
}
.trainer-detail-heading time {
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
  font-variant-numeric: tabular-nums;
}
.trainer-detail-facts {
  display: flex;
  flex-wrap: wrap;
  gap: var(--fono-space-4) var(--fono-space-6);
  margin: 0;
}
.trainer-detail-facts > div {
  display: grid;
  gap: var(--fono-space-1);
}
.trainer-detail-facts dt {
  display: flex;
  align-items: center;
  gap: var(--fono-space-1);
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
}
.trainer-detail-facts dd {
  margin: 0;
  color: var(--fono-text);
  font-size: var(--fono-type-lg);
  font-weight: 600;
}
.trainer-detail-facts small {
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
  font-weight: 400;
}
.trainer-technical {
  font-size: var(--fono-type-sm);
  overflow-wrap: anywhere;
}
.trainer-detail-card {
  min-width: 0;
  background: var(--fono-canvas);
}
.trainer-detail-card :deep(.wl-card__caption) {
  padding: var(--fono-space-4) var(--fono-space-4) 0;
}
.trainer-detail-card :deep(.wl-card__content) {
  padding: var(--fono-space-4);
}
.trainer-original-heading {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: var(--fono-space-3);
}
h3 {
  font-size: var(--fono-type-lg);
  margin: 0;
}
.trainer-original-text {
  max-height: var(--fono-text-max-height);
  overflow: auto;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  color: var(--fono-text);
  font-size: var(--fono-type-md);
  line-height: 1.8;
}
.transcript-marker {
  color: var(--marker-color);
  background: transparent;
  text-decoration-line: underline;
  text-decoration-thickness: from-font;
  text-underline-offset: 0.15em;
}
.transcript-marker-number {
  margin-inline: var(--fono-space-1);
  color: var(--marker-color);
  font-size: var(--fono-type-xs);
  font-weight: 600;
  line-height: 1;
  vertical-align: super;
  user-select: none;
}
.transcript-marker-number::after {
  content: attr(data-number);
}
[data-category="filler"] {
  --marker-color: var(--fono-trainer-filler);
}
[data-category="repetition"] {
  --marker-color: var(--fono-trainer-repetition);
}
[data-category="selfCorrection"] {
  --marker-color: var(--fono-trainer-correction);
}
[data-category="unfinished"] {
  --marker-color: var(--fono-trainer-unfinished);
}
.trainer-category-counts {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--fono-space-1) var(--fono-space-4);
  margin: var(--fono-space-3) 0 0;
  padding-top: var(--fono-space-3);
  border-top: 1px solid var(--fono-border-soft);
  color: var(--fono-secondary);
  font-size: var(--fono-type-md);
  line-height: 1.5;
}
.trainer-category-counts > div {
  display: flex;
  align-items: center;
  gap: var(--fono-space-2);
  min-width: 0;
}
.trainer-category-counts dt {
  display: flex;
  align-items: center;
  gap: var(--fono-space-2);
  min-width: 0;
  color: var(--marker-color);
}
.trainer-category-counts dt span {
  overflow-wrap: anywhere;
}
.trainer-category-counts dd {
  flex-shrink: 0;
  margin: 0;
  color: var(--marker-color);
  font-size: var(--fono-type-lg);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
.trainer-category-dot {
  flex-shrink: 0;
  width: var(--fono-space-2);
  height: var(--fono-space-2);
  border-radius: var(--fono-radius-sm);
  background: var(--marker-color);
}
</style>
