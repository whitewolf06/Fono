<script setup lang="ts">
import { computed } from "vue";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import type { DictationMode } from "../../../shared/domain/contracts";
const workspace = useWorkspace();
const { run, busy, error } = useFeedback();
const active = computed(() =>
  ["listening", "silence", "transcribing", "processing"].includes(
    workspace.state.phase,
  ),
);
function choose(mode: DictationMode) {
  if (mode === workspace.state.preferences.dictationMode || active.value)
    return;
  void run(() => workspace.settings.save({ dictationMode: mode }));
}
</script>
<template>
  <div class="dictation-mode-control">
    <div class="segmented small" aria-label="Режим диктовки">
      <button
        v-for="mode in ['standard', 'live'] as const"
        :key="mode"
        type="button"
        :class="{
          selected: workspace.state.preferences.dictationMode === mode,
        }"
        :aria-pressed="workspace.state.preferences.dictationMode === mode"
        :disabled="active || busy"
        @click="choose(mode)"
      >
        {{ mode === "live" ? "Живая диктовка" : "Обычная" }}
      </button>
    </div>
    <small>{{
      workspace.state.preferences.dictationMode === "live"
        ? "Устойчивые фрагменты · без обработки ИИ"
        : "Готовый текст после завершения"
    }}</small>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
  </div>
</template>
