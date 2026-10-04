<script setup lang="ts">
import { ref } from "vue";
import type {
  Phase,
  Preferences,
  LiveDictation,
} from "../../../shared/domain/contracts";
import type {
  PendingDictation,
  PendingDictationRequest,
} from "../../../shared/domain/processing";
import { liveStatus, VoiceWave } from "../../dictation";
import { phaseLabels } from "../../../shared/application/workspace";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import PendingDictationActions from "../../../shared/presentation/PendingDictationActions.vue";
withDefaults(
  defineProps<{
    preferences: Preferences;
    phase?: Phase;
    level?: number;
    seconds?: number;
    interactive?: boolean;
    canFinish?: boolean;
    elapsed?: number;
    live?: LiveDictation | null;
    pending?: PendingDictation | null;
  }>(),
  { phase: "listening", level: 0.6, seconds: 3, elapsed: 12, canFinish: true },
);
defineEmits<{
  finish: [];
  cancel: [];
  resume: [];
  resolve: [request: PendingDictationRequest];
  copy: [text: string];
}>();
const help = ref(false);
</script>
<template>
  <div class="overlay-preview-wrap">
    <div
      class="overlay-preview"
      :class="{
        compact: preferences.overlayCompact,
        disabled: !preferences.overlayEnabled,
        expanded: !!pending,
        'show-help': help,
      }"
      :style="{
        zoom: preferences.overlayScale / 100,
        opacity: preferences.overlayOpacity / 100,
      }"
    >
      <div v-if="pending" class="overlay-pending-header">
        <AppIcon
          :name="pending.insertionBlocked ? 'warn' : 'sparkle'"
          :size="18"
        />
        <strong>{{
          pending.phase === "processing"
            ? "Обрабатываю текст"
            : pending.insertionBlocked
              ? "Текст сохранён для копирования"
              : "Текст готов · выберите действие"
        }}</strong>
        <button
          class="overlay-control"
          aria-label="Подсказка о кнопках"
          :aria-expanded="help"
          @click="help = !help"
        >
          <AppIcon name="info" :size="16" />
        </button>
      </div>
      <template v-else>
        <AppIcon
          :name="
            phase === 'error'
              ? 'warn'
              : phase === 'done'
                ? 'check'
                : 'microphone'
          "
          :size="20"
        />
        <div v-if="!help" class="overlay-body">
          <strong>{{
            preferences.overlayEnabled
              ? live
                ? liveStatus(live)
                : phaseLabels[phase]
              : "Индикатор выключен"
          }}</strong>
          <VoiceWave
            v-if="!preferences.overlayCompact"
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
          <small
            v-if="
              phase === 'silence' ||
              (live?.phase === 'listening' && seconds > 0)
            "
            >{{ live ? "Фрагмент" : "Завершение" }} через
            {{ seconds }} сек</small
          >
          <small v-else-if="!preferences.overlayCompact">{{
            phase === "error"
              ? "Проверьте настройки"
              : phase === "listening"
                ? Math.floor(elapsed / 60)
                    .toString()
                    .padStart(2, "0") +
                  ":" +
                  Math.floor(elapsed % 60)
                    .toString()
                    .padStart(2, "0")
                : "Fono"
          }}</small>
        </div>
        <div v-if="help" class="overlay-inline-help">
          {{
            preferences.overlayCompact
              ? "■ Завершить · × Отмена"
              : preferences.hotkeyMode === "toggle"
                ? "Повторное нажатие или ■ — завершить. × — отмена."
                : "Отпустить или ■ — завершить. × — отмена."
          }}
        </div>
        <button
          v-if="
            interactive &&
            live?.phase === 'listening' &&
            ['none', 'paused_focus'].includes(live.insertionState)
          "
          class="overlay-control dictation-action--insert"
          aria-label="Продолжить вставку в выбранное поле"
          @click="$emit('resume')"
        >
          <AppIcon name="play" :size="15" />
        </button>
        <button
          v-if="
            interactive &&
            canFinish &&
            (phase === 'listening' || phase === 'silence')
          "
          class="overlay-control dictation-action--insert"
          title="Завершить диктовку"
          aria-label="Завершить диктовку"
          @click="$emit('finish')"
        >
          <AppIcon name="stop" :size="15" />
        </button>
        <button
          class="overlay-control"
          title="Подсказка о кнопках"
          aria-label="Подсказка о кнопках"
          :aria-expanded="help"
          @click="help = !help"
        >
          <AppIcon name="info" :size="15" />
        </button>
        <button
          v-if="interactive"
          class="overlay-control dictation-action--cancel"
          title="Отменить или закрыть индикатор"
          aria-label="Отменить или закрыть индикатор"
          @click="$emit('cancel')"
        >
          <AppIcon name="x" :size="15" />
        </button>
      </template>
      <div v-if="pending && help" class="overlay-legend">
        <p class="dictation-action--insert">Исходный — вставить без ИИ.</p>
        <p class="dictation-action--process">
          Обработка — очистить, оформить, составить задачу или письмо.
        </p>
        <p class="dictation-action--translate">
          Перевод — обработать и перевести на выбранный язык.
        </p>
        <p class="dictation-action--cancel">
          × Отменить без вставки. Копировать — сохранить текст в буфере.
        </p>
        <small>Выбор действует только для этой диктовки.</small>
        <button class="dictation-action" @click="help = false">
          Назад к тексту
        </button>
      </div>
      <PendingDictationActions
        v-else-if="pending"
        :pending="pending"
        @resolve="(request) => $emit('resolve', request)"
        @copy="(text) => $emit('copy', text)"
      />
    </div>
  </div>
</template>
