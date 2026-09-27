<script setup lang="ts">
import { computed } from "vue";
import type { VoicePhase } from "../domain/voice";

const props = defineProps<{
  phase: VoicePhase;
  label: string;
  level?: number;
}>();

const bars = Array.from({ length: 51 }, (_, index) => {
  const distance = Math.abs(index - 25) / 25;
  const envelope = Math.pow(Math.max(0, 1 - distance), 1.35);
  const ripple = Math.sin(index * 1.45) * 11 + Math.sin(index * 0.58) * 8;

  return {
    height: Math.max(20, Math.round(30 + envelope * 125 + ripple)),
    delay: -(index * 0.067),
  };
});

const energy = computed(() => {
  if (props.level !== undefined) {
    return 0.45 + Math.min(1, Math.max(0, props.level)) * 1.45;
  }

  if (props.phase === "listening") return 1.55;
  if (props.phase === "transcribing" || props.phase === "processing")
    return 1.05;
  if (props.phase === "error") return 0.35;
  return 1.08;
});
</script>

<template>
  <div class="v3-wave-stage">
    <div
      class="v3-wave-visual"
      :class="'is-' + phase"
      :style="{ '--fono-wave-energy': energy }"
      aria-hidden="true"
    >
      <span class="v3-wave-aura"></span>
      <span class="v3-wave-contour v3-wave-contour--outer"></span>
      <span class="v3-wave-contour v3-wave-contour--inner"></span>
      <span class="v3-wave-axis"></span>
      <div class="v3-wave-bars">
        <span
          v-for="(bar, index) in bars"
          :key="index"
          class="v3-wave-bar"
          :style="{
            '--wave-bar-height': bar.height + 'px',
            '--wave-delay': bar.delay + 's',
          }"
        ></span>
      </div>
    </div>
    <span class="v3-status" :class="'is-' + phase" role="status">
      <span class="v3-status-dot"></span>{{ label }}
    </span>
  </div>
</template>
