<script setup lang="ts">
import { computed } from "vue";
import type {
  Phase,
  Preferences,
  LiveDictation,
} from "../../../shared/domain/contracts";
import { liveStatus, VoiceWave } from "../../dictation";
import { phaseLabels } from "../../../shared/application/workspace";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
const props = defineProps<{
  preferences: Preferences;
  phase: Phase;
  level: number;
  elapsed: number;
  seconds: number;
  live?: LiveDictation | null;
  interactive?: boolean;
  canFinish?: boolean;
  saving?: boolean;
  help: boolean;
  helpId: string;
}>();
defineEmits<{ finish: []; cancel: []; resume: []; help: [] }>();
const recording = computed(() =>
  ["listening", "silence"].includes(props.phase),
);
const clock = computed(
  () =>
    `${Math.floor(props.elapsed / 60)
      .toString()
      .padStart(2, "0")}:${Math.floor(props.elapsed % 60)
      .toString()
      .padStart(2, "0")}`,
);
const label = computed(() =>
  !props.preferences.overlayEnabled
    ? "Индикатор выключен"
    : props.live
      ? liveStatus(props.live)
      : phaseLabels[props.phase],
);
</script>
<template>
  <div class="overlay-toolbar">
    <div class="overlay-status-row">
      <AppIcon
        :name="
          phase === 'error' ? 'warn' : phase === 'done' ? 'check' : 'microphone'
        "
        :size="20"
        class="overlay-status-icon"
      />
      <strong class="overlay-status" :title="label">{{ label }}</strong>
      <small v-if="recording" class="overlay-time">{{ clock }}</small>
    </div>
    <VoiceWave
      v-if="!preferences.overlayCompact && !live"
      :phase="phase"
      :level="level"
    />
    <small
      v-if="live && !preferences.overlayCompact"
      class="overlay-live-text"
      :title="live.committedText + ' ' + live.draftText"
      >{{ live.committedText.slice(-80)
      }}<span class="live-draft"> {{ live.draftText }}</span></small
    >
    <div class="overlay-controls" data-overlay-interactive>
      <small
        v-if="
          phase === 'silence' || (live?.phase === 'listening' && seconds > 0)
        "
        class="overlay-countdown"
        >Пауза · {{ seconds }} с</small
      >
      <button
        type="button"
        v-if="
          interactive &&
          live?.phase === 'listening' &&
          ['none', 'paused_focus'].includes(live.insertionState)
        "
        class="dictation-action dictation-action--insert"
        aria-label="Продолжить вставку в выбранное поле"
        @click="$emit('resume')"
      >
        <AppIcon name="play" :size="14" />Вставка
      </button>
      <button
        type="button"
        v-if="interactive && canFinish && recording"
        class="dictation-action overlay-finish"
        :disabled="saving"
        title="Завершить запись и распознать речь"
        @click="$emit('finish')"
      >
        <AppIcon name="stop" :size="14" class="overlay-stop-symbol" />Завершить
      </button>
      <button
        type="button"
        v-if="interactive"
        class="dictation-action overlay-cancel"
        :title="
          recording || phase === 'processing' || phase === 'transcribing'
            ? 'Отменить диктовку без вставки'
            : 'Закрыть индикатор'
        "
        @click="$emit('cancel')"
      >
        <AppIcon name="x" :size="14" />{{
          recording || phase === "processing" || phase === "transcribing"
            ? "Отмена"
            : "Закрыть"
        }}
      </button>
      <button
        type="button"
        class="overlay-control overlay-help-toggle"
        title="Что означают кнопки"
        aria-label="Пояснения к кнопкам"
        :aria-expanded="help"
        :aria-controls="helpId"
        @click="$emit('help')"
      >
        <AppIcon name="info" :size="16" />
      </button>
    </div>
  </div>
</template>
