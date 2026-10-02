<script setup lang="ts">
import type { Phase, Preferences } from "../../../shared/domain/contracts";
import { phaseLabels } from "../../../shared/application/workspace";
import { VoiceWave } from "../../dictation";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
withDefaults(
  defineProps<{
    preferences: Preferences;
    phase?: Phase;
    level?: number;
    seconds?: number;
    interactive?: boolean;
    canFinish?: boolean;
    elapsed?: number;
  }>(),
  { phase: "listening", level: 0.6, seconds: 3, elapsed: 12, canFinish: true },
);
defineEmits<{ finish: []; cancel: [] }>();
</script>
<template>
  <div class="overlay-preview-wrap">
    <div
      class="overlay-preview"
      :class="{
        compact: preferences.overlayCompact,
        disabled: !preferences.overlayEnabled,
      }"
      :style="{
        zoom: preferences.overlayScale / 100,
        opacity: preferences.overlayOpacity / 100,
      }"
    >
      <AppIcon
        :name="
          phase === 'error' ? 'warn' : phase === 'done' ? 'check' : 'microphone'
        "
        :size="20"
      />
      <div class="overlay-body">
        <strong>{{
          preferences.overlayEnabled ? phaseLabels[phase] : "Индикатор выключен"
        }}</strong
        ><VoiceWave
          v-if="!preferences.overlayCompact"
          :phase="phase"
          :level="level"
        /><small v-if="phase === 'silence'"
          >Завершение через {{ seconds }} сек</small
        ><small v-else-if="!preferences.overlayCompact">{{
          phase === "error"
            ? "Проверьте микрофон"
            : phase === "listening"
              ? Math.floor(elapsed / 60)
                  .toString()
                  .padStart(2, "0") +
                ":" +
                (elapsed % 60).toString().padStart(2, "0") +
                " · " +
                preferences.hotkey
              : "Fono"
        }}</small>
      </div>
      <button
        v-if="
          interactive &&
          canFinish &&
          (phase === 'listening' || phase === 'silence')
        "
        class="overlay-control"
        aria-label="Завершить диктовку"
        @click="$emit('finish')"
      >
        <AppIcon name="stop" :size="15" /></button
      ><button
        v-if="interactive"
        class="overlay-control"
        aria-label="Отменить или закрыть индикатор"
        @click="$emit('cancel')"
      >
        <AppIcon name="x" :size="15" />
      </button>
    </div>
  </div>
</template>
