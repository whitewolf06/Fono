<script setup lang="ts">
import { computed } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import {
  useWorkspace,
  phaseLabels,
} from "../../../shared/application/workspace";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import VoiceWave from "./VoiceWave.vue";
import { liveStatus } from "../domain/live";
import { useFeedback } from "../../../shared/application/feedback";
import PendingDictationActions from "../../../shared/presentation/PendingDictationActions.vue";
const { run } = useFeedback();
const workspace = useWorkspace();
const live = computed(() => workspace.state.live);
const liveMode = computed(
  () => workspace.state.preferences.dictationMode === "live",
);
const status = computed(() =>
  liveMode.value && live.value
    ? liveStatus(live.value)
    : phaseLabels[workspace.state.phase],
);
const canResume = computed(
  () =>
    live.value?.phase === "listening" &&
    ["paused_focus", "none"].includes(live.value.insertionState),
);
const recording = computed(() =>
  ["listening", "silence"].includes(workspace.state.phase),
);
const working = computed(() =>
  ["transcribing", "processing"].includes(workspace.state.phase),
);
</script>
<template>
  <section class="record-area" aria-label="Диктовка">
    <VoiceWave
      :phase="workspace.state.phase"
      :level="workspace.state.audioLevel"
    />
    <PendingDictationActions
      v-if="workspace.state.pendingDictation"
      :pending="workspace.state.pendingDictation"
      @resolve="
        (request) => run(() => workspace.dictation.resolvePending(request))
      "
      @copy="(text) => run(() => workspace.copy(text), 'Скопировано')"
    />
    <div v-else class="record-controls">
      <span class="record-status" role="status"
        >{{ status
        }}<span v-if="recording" class="mono"
          >{{
            Math.floor(workspace.state.elapsed / 60)
              .toString()
              .padStart(2, "0")
          }}:{{
            Math.floor(workspace.state.elapsed % 60)
              .toString()
              .padStart(2, "0")
          }}</span
        ></span
      ><WlButton
        size="sm"
        :variant="recording ? 'soft-danger' : 'soft'"
        :loading="working"
        @click="
          run(() =>
            recording
              ? workspace.dictation.finish()
              : workspace.dictation.start(),
          )
        "
        ><template #icon
          ><AppIcon
            :name="recording ? 'stop' : 'microphone'"
            :size="16" /></template
        >{{
          recording ? "Завершить" : working ? "Обработка" : "Начать запись"
        }}</WlButton
      ><WlButton
        v-if="recording || working"
        size="sm"
        variant="ghost"
        @click="run(() => workspace.dictation.cancel())"
        >Отмена</WlButton
      >
    </div>
    <p v-if="liveMode && !recording && !working" class="live-notice">
      Выберите поле в приложении и нажмите
      {{ workspace.state.preferences.hotkey }}. Кнопка записи здесь выводит
      текст в Fono.
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
    <p v-if="workspace.state.error" class="record-error" role="alert">
      {{ workspace.state.error }}
      <RouterLink
        :to="
          workspace.state.microphoneAvailable
            ? '/settings/processing'
            : '/settings/audio'
        "
        >Настройки</RouterLink
      >
    </p>
  </section>
</template>
