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
function drag(event: PointerEvent) {
  if (event.target instanceof Element && !event.target.closest("button"))
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
      interactive
      :can-finish="
        !overlay.state.source ||
        ['ui', 'wake_word'].includes(overlay.state.source)
      "
      @finish="overlay.finish"
      @cancel="overlay.cancel"
    />
    <span v-if="overlay.state.error" class="sr-only" role="alert">{{
      overlay.state.error
    }}</span>
  </div>
</template>
