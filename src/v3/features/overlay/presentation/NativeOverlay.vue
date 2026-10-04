<script setup lang="ts">
import { computed, onUnmounted } from "vue";
import OverlayPreview from "./OverlayPreview.vue";
import { createNativeOverlay } from "../infrastructure/nativeOverlay";
const overlay = createNativeOverlay();
const phase = computed(() =>
  overlay.state.preview && overlay.state.phase === "idle"
    ? "listening"
    : overlay.state.phase,
);
const pending = computed(() =>
  overlay.state.pending && overlay.state.error
    ? {
        ...overlay.state.pending,
        error: overlay.state.pending.error || overlay.state.error,
      }
    : overlay.state.pending,
);
function drag(event: PointerEvent) {
  if (
    event.button === 0 &&
    event.target instanceof Element &&
    !event.target.closest(
      "button, select, input, a, [data-overlay-interactive]",
    )
  )
    void overlay.drag();
}
onUnmounted(overlay.dispose);
</script>
<template>
  <div class="native-overlay" @pointerdown="drag">
    <OverlayPreview
      :preferences="overlay.state.preferences"
      :phase="phase"
      :level="overlay.state.level"
      :seconds="overlay.state.seconds"
      :elapsed="overlay.state.elapsed"
      :live="overlay.state.live"
      :pending="pending"
      interactive
      @finish="overlay.finish"
      @cancel="overlay.cancel"
      @resume="overlay.resume"
      @resolve="overlay.resolve"
      @copy="overlay.copy"
    />
    <p
      v-if="overlay.state.error && !pending"
      class="native-overlay-error"
      role="alert"
    >
      {{ overlay.state.error }}
    </p>
  </div>
</template>
