<script setup lang="ts">
import { computed, onUnmounted } from "vue";
import OverlayPreview from "./OverlayPreview.vue";
import { createNativeOverlay } from "../infrastructure/nativeOverlay";
import SelectionOverlay from "../../selection/presentation/SelectionOverlay.vue";
const overlay = createNativeOverlay();
const showSelection = computed(
  () =>
    overlay.selection.state.current &&
    !overlay.state.pending &&
    !["listening", "silence", "transcribing", "processing"].includes(
      overlay.state.phase,
    ),
);
const phase = computed(() =>
  overlay.state.preview && overlay.state.phase === "idle"
    ? "listening"
    : overlay.state.phase,
);
const pending = computed(() =>
  overlay.state.pending && overlay.state.error
    ? {
        ...overlay.state.pending,
        error: overlay.state.error || overlay.state.pending.error,
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
      v-if="!showSelection"
      :preferences="overlay.state.preferences"
      :phase="phase"
      :outcome="overlay.state.dictationOutcome"
      :level="overlay.state.level"
      :seconds="overlay.state.seconds"
      :elapsed="overlay.state.elapsed"
      :live="overlay.state.live"
      :pending="pending"
      :copying="overlay.state.copying"
      :processing-choice="
        overlay.state.processingChoice ?? overlay.state.sessionChoice
      "
      :recognition="overlay.state.recognition"
      :application-profile="overlay.state.applicationProfile"
      :processing-saving="
        overlay.processing.pending || overlay.processing.saving
      "
      :processing-error="overlay.processing.error"
      :error="pending ? undefined : overlay.state.error"
      interactive
      @finish="overlay.finish"
      @cancel="overlay.cancel"
      @resume="overlay.resume"
      @resolve="overlay.resolve"
      @copy="overlay.copy"
      @processing-change="overlay.chooseProcessing"
      @help-change="overlay.setHelp"
    />
    <SelectionOverlay
      v-else
      :port="overlay.selection"
      :preferences="overlay.state.preferences"
      native
    />
  </div>
</template>
