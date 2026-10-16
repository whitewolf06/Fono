<script setup lang="ts">
import { computed } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
const workspace = useWorkspace();
const { run } = useFeedback();
const live = computed(() => workspace.state.live);
const liveMode = computed(
  () => workspace.state.preferences.dictationMode === "live",
);
const recording = computed(() =>
  ["listening", "silence"].includes(workspace.state.phase),
);
const working = computed(() =>
  ["transcribing", "processing"].includes(workspace.state.phase),
);
const canResume = computed(
  () =>
    live.value?.phase === "listening" &&
    ["paused_focus", "none"].includes(live.value.insertionState),
);
</script>
<template>
  <p v-if="liveMode && !recording && !working" class="live-notice">
    Выберите поле в приложении и нажмите
    {{ workspace.state.preferences.hotkey }}. Кнопка записи здесь выводит текст
    в Fono.
  </p>
  <p
    v-if="live && recording"
    class="live-notice"
    :class="{
      warning: live.insertionState === 'failed' || live.lagMs > 4000,
    }"
    role="status"
  >
    {{
      live.warning ||
      (live.insertionState === "paused_focus"
        ? "Поле изменилось. Распознавание продолжается, текст ждёт вставки."
        : live.insertionState === "none"
          ? "Выберите поле и нажмите «Продолжить вставку» в индикаторе. Или завершите и скопируйте текст."
          : live.insertionState === "failed"
            ? "Остаток текста сохранён. Проверьте последний фрагмент в поле перед ручным копированием. Автоматическая вставка остановлена."
            : live.lagMs > 4000
              ? "Аудио сохраняется, подтверждённые фрагменты появятся по порядку."
              : "Паузы разделяют фразы. Завершите запись кнопкой или горячей клавишей.")
    }}
  </p>
  <p v-if="canResume && !workspace.native" class="live-notice">
    <WlButton
      size="xs"
      variant="ghost"
      @click="run(() => workspace.dictation.resumeInsertion())"
      >Продолжить вставку · демо</WlButton
    >
  </p>
</template>
