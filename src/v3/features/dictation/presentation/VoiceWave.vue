<script setup lang="ts">
import { computed, useId } from "vue";
import type { Phase } from "../../../shared/domain/contracts";
import { voiceWaveAppearance } from "./voiceWaveAppearance";
const props = withDefaults(defineProps<{ level?: number; phase?: Phase }>(), {
  level: 0,
  phase: "idle",
});
const id = useId().replace(/:/g, "");
const appearance = computed(() =>
  voiceWaveAppearance(props.phase, props.level),
);
const style = computed(() => ({
  "--wave-amplitude": appearance.value.amplitude,
  "--wave-opacity": appearance.value.opacity,
  "--wave-stroke": `${appearance.value.strokeWidth}px`,
}));
</script>
<template>
  <div class="voice-wave" :data-phase="phase" :style="style" aria-hidden="true">
    <svg
      class="voice-wave-canvas"
      viewBox="0 0 1000 220"
      preserveAspectRatio="none"
    >
      <defs>
        <linearGradient :id="id + '-fade'">
          <stop offset="0" stop-color="white" stop-opacity="0" />
          <stop offset=".16" stop-color="white" />
          <stop offset=".84" stop-color="white" />
          <stop offset="1" stop-color="white" stop-opacity="0" />
        </linearGradient>
        <mask :id="id + '-mask'">
          <rect width="1000" height="220" :fill="'url(#' + id + '-fade)'" />
        </mask>
        <filter
          :id="id + '-glow'"
          x="-10%"
          y="-100%"
          width="120%"
          height="300%"
        >
          <feGaussianBlur :stdDeviation="appearance.glow" />
          <feMerge>
            <feMergeNode />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>
      <g :mask="'url(#' + id + '-mask)'">
        <g class="voice-wave-amplitude" :filter="'url(#' + id + '-glow)'">
          <path
            class="voice-ribbon voice-ribbon-one"
            vector-effect="non-scaling-stroke"
            d="M-200 110 Q-120 -70 -40 110 T120 110 T280 110 T440 110 T600 110 T760 110 T920 110 T1080 110 T1240 110"
          />
          <path
            class="voice-ribbon voice-ribbon-two"
            vector-effect="non-scaling-stroke"
            d="M-240 110 Q-135 264 -30 110 T180 110 T390 110 T600 110 T810 110 T1020 110 T1230 110"
          />
          <path
            class="voice-ribbon voice-ribbon-three"
            vector-effect="non-scaling-stroke"
            d="M-260 110 Q-135 -50 -10 110 T240 110 T490 110 T740 110 T990 110 T1240 110"
          />
        </g>
      </g>
    </svg>
  </div>
</template>

<style scoped>
:where(.voice-wave) {
  width: min(100%, var(--fono-wave-width));
  height: var(--fono-wave-height);
  margin: 0 auto;
  overflow: visible;
  pointer-events: none;
  color: var(--fono-accent);
  opacity: var(--wave-opacity);
  transition: opacity var(--fono-motion-fast) ease;
}
.voice-wave-canvas {
  display: block;
  width: 100%;
  height: 100%;
  overflow: hidden;
}
.voice-wave-amplitude {
  transform-origin: 500px 110px;
  transform: scaleY(var(--wave-amplitude));
  transition: transform var(--fono-motion-fast) ease-out;
}
.voice-ribbon {
  fill: none;
  stroke: currentColor;
  stroke-width: var(--wave-stroke);
  stroke-linecap: round;
  animation: voice-wave-flow var(--fono-wave-idle) ease-in-out infinite
    alternate;
}
.voice-ribbon-two {
  color: var(--fono-electric-soft);
  opacity: 0.78;
  animation-direction: alternate-reverse;
  animation-duration: calc(var(--fono-wave-idle) * 1.25);
}
.voice-ribbon-three {
  opacity: 0.5;
  animation-delay: -2s;
  animation-duration: calc(var(--fono-wave-idle) * 0.8);
}
.voice-wave[data-phase="listening"] .voice-ribbon,
.voice-wave[data-phase="silence"] .voice-ribbon {
  animation-duration: var(--fono-wave-recording);
}
.voice-wave[data-phase="processing"] .voice-ribbon,
.voice-wave[data-phase="transcribing"] .voice-ribbon {
  animation-duration: var(--fono-wave-processing);
}
.voice-wave[data-phase="error"] {
  color: var(--fono-error);
}
.voice-wave[data-phase="error"] .voice-ribbon-two {
  color: inherit;
}
@keyframes voice-wave-flow {
  from {
    transform: translateX(calc(var(--fono-wave-drift) * -1));
  }
  to {
    transform: translateX(var(--fono-wave-drift));
  }
}
@media (prefers-reduced-motion: reduce) {
  .voice-ribbon {
    animation: none;
  }
  .voice-wave,
  .voice-wave-amplitude {
    transition: none;
  }
}
</style>
