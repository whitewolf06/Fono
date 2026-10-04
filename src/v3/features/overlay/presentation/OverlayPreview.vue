<script setup lang="ts">
import { computed, ref, useId, watch } from "vue";
import type {
  Phase,
  Preferences,
  LiveDictation,
} from "../../../shared/domain/contracts";
import type {
  PendingDictation,
  PendingDictationRequest,
  OverlayProcessingChoice,
} from "../../../shared/domain/processing";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import PendingDictationActions from "../../../shared/presentation/PendingDictationActions.vue";
import ProcessingChoiceControls from "../../../shared/presentation/ProcessingChoiceControls.vue";
import OverlayToolbar from "./OverlayToolbar.vue";
import OverlayHelp from "./OverlayHelp.vue";
const props = withDefaults(
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
    processingChoice?: OverlayProcessingChoice | null;
    processingSaving?: boolean;
    processingError?: string;
    error?: string;
  }>(),
  { phase: "listening", level: 0.6, seconds: 3, elapsed: 12, canFinish: true },
);
const emit = defineEmits<{
  finish: [];
  cancel: [];
  resume: [];
  resolve: [request: PendingDictationRequest];
  copy: [text: string];
  processingChange: [choice: OverlayProcessingChoice];
  helpChange: [open: boolean];
}>();
const help = ref(false);
const helpId = useId();
const rememberChoice = computed(
  () =>
    props.preferences.hotkeyMode === "toggle" &&
    props.preferences.overlayQuickProcessing &&
    props.preferences.processingEnabled,
);
const quick = computed(
  () =>
    rememberChoice.value &&
    ["listening", "silence"].includes(props.phase) &&
    !props.pending,
);
const choice = computed<OverlayProcessingChoice>(
  () =>
    props.processingChoice ?? {
      preset: props.pending?.preset ?? props.preferences.processingMode,
      targetLanguage: props.pending
        ? props.pending.targetLanguage
        : props.preferences.processingTranslation === "none"
          ? null
          : props.preferences.processingTranslation,
    },
);
watch(help, (open) => emit("helpChange", open));
</script>
<template>
  <div class="overlay-preview-wrap">
    <div
      class="overlay-preview"
      :class="{
        compact: preferences.overlayCompact,
        disabled: !preferences.overlayEnabled,
        expanded: !!pending,
        quick,
        'show-help': help,
      }"
      :style="{
        zoom: preferences.overlayScale / 100,
        '--fono-overlay-zoom': preferences.overlayScale / 100,
        opacity: preferences.overlayOpacity / 100,
      }"
    >
      <template v-if="pending">
        <div class="overlay-pending-header">
          <AppIcon
            :name="pending.insertionBlocked ? 'warn' : 'sparkle'"
            :size="18"
          />
          <strong>{{
            pending.phase === "processing"
              ? "Обрабатываю текст"
              : pending.insertionBlocked
                ? "Текст доступен для копирования"
                : "Текст готов · выберите действие"
          }}</strong>
          <button
            type="button"
            class="overlay-control overlay-help-toggle"
            title="Что означают кнопки"
            aria-label="Пояснения к кнопкам"
            :aria-expanded="help"
            :aria-controls="helpId"
            @click="help = !help"
          >
            <AppIcon name="info" :size="16" />
          </button>
        </div>
        <PendingDictationActions
          :pending="pending"
          :choice="rememberChoice ? choice : undefined"
          :saving="processingSaving"
          :remember-choice="rememberChoice"
          @choice="(value) => emit('processingChange', value)"
          @resolve="(request) => emit('resolve', request)"
          @copy="(text) => emit('copy', text)"
        />
      </template>
      <OverlayToolbar
        v-else
        :preferences="preferences"
        :phase="phase"
        :level="level"
        :elapsed="elapsed"
        :seconds="seconds"
        :live="live"
        :interactive="interactive"
        :can-finish="canFinish"
        :saving="processingSaving"
        :help="help"
        :help-id="helpId"
        @finish="emit('finish')"
        @cancel="emit('cancel')"
        @resume="emit('resume')"
        @help="help = !help"
      />
      <ProcessingChoiceControls
        v-if="quick"
        :choice="choice"
        :saving="processingSaving"
        remember
        @change="(value) => emit('processingChange', value)"
      />
      <p
        v-if="processingError || error"
        class="overlay-error"
        role="alert"
        data-overlay-interactive
      >
        {{ processingError || error }}
      </p>
      <OverlayHelp
        v-if="help"
        :id="helpId"
        :preferences="preferences"
        :pending="!!pending"
      />
    </div>
  </div>
</template>
