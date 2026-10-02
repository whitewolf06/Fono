<script setup lang="ts">
import { computed, useId } from "vue";
import type { Phase } from "../../../shared/domain/contracts";
const props = withDefaults(defineProps<{ level?: number; phase?: Phase }>(), {
  level: 0,
  phase: "idle",
});
const id = useId().replace(/:/g, "");
const amplitude = computed(() =>
  ["listening", "silence"].includes(props.phase)
    ? 0.3 + Math.max(0, Math.min(1, props.level)) * 1.05
    : props.phase === "processing" || props.phase === "transcribing"
      ? 0.7
      : 0.32,
);
</script>
<template>
  <div
    class="voice-wave"
    :data-phase="phase"
    :style="{ '--wave-amplitude': amplitude }"
    aria-hidden="true"
  >
    <svg viewBox="0 0 680 100" preserveAspectRatio="xMidYMid meet">
      <defs>
        <linearGradient :id="id + '-fade'">
          <stop offset="0" stop-color="white" stop-opacity="0" />
          <stop offset=".2" stop-color="white" />
          <stop offset=".8" stop-color="white" />
          <stop offset="1" stop-color="white" stop-opacity="0" />
        </linearGradient>
        <mask :id="id + '-mask'">
          <rect width="680" height="100" :fill="'url(#' + id + '-fade)'" />
        </mask>
      </defs>
      <g :mask="'url(#' + id + '-mask)'">
        <g class="ribbon-amplitude">
          <path
            class="ribbon ribbon-one"
            d="M-200 50 Q-130 4 -60 50 T80 50 T220 50 T360 50 T500 50 T640 50 T780 50 T920 50"
          />
          <path
            class="ribbon ribbon-two"
            d="M-180 50 Q-100 92 -20 50 T140 50 T300 50 T460 50 T620 50 T780 50 T940 50"
          />
          <path
            class="ribbon ribbon-three"
            d="M-180 50 Q-80 12 20 50 T220 50 T420 50 T620 50 T820 50"
          />
        </g>
      </g>
    </svg>
  </div>
</template>
