<script setup lang="ts">
import { computed } from "vue";
import type { DictationOutcomeKind } from "../../../shared/domain/dictationOutcome";
import { message } from "../../../shared/domain/messages";
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
import type { IndicatorChoice, IndicatorState } from "../domain/indicator";
import {
  previewRecognitionCapability,
  type OverlayRecognitionCapability,
} from "../../../shared/domain/recognition";
import OverlayIndicator from "./OverlayIndicator.vue";
const props = withDefaults(
  defineProps<{
    preferences: Preferences;
    phase?: Phase;
    outcome?: DictationOutcomeKind;
    level?: number;
    seconds?: number;
    interactive?: boolean;
    canFinish?: boolean;
    elapsed?: number;
    live?: LiveDictation | null;
    pending?: PendingDictation | null;
    processingChoice?: OverlayProcessingChoice | null;
    recognition?: OverlayRecognitionCapability;
    applicationProfile?: string | null;
    processingSaving?: boolean;
    processingError?: string;
    copying?: boolean;
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
const choice = computed<OverlayProcessingChoice>(
  () =>
    props.processingChoice ?? {
      preset: props.pending?.preset ?? props.preferences.processingMode,
      targetLanguage:
        props.pending?.targetLanguage ??
        (props.preferences.processingTranslation === "none"
          ? null
          : props.preferences.processingTranslation),
      processingEnabled:
        props.pending?.processingEnabled ?? props.preferences.processingEnabled,
      translationEnabled:
        props.pending?.translationEnabled ??
        props.preferences.processingTranslationEnabled,
    },
);
const indicator = computed<IndicatorState>(() => {
  const pending = props.pending;
  const error = props.processingError || pending?.error || props.error || "";
  const phase = pending
    ? pending.phase === "processing"
      ? "processing"
      : error
        ? "error"
        : "ready"
    : props.outcome === "no_speech" || props.outcome === "superseded"
      ? "closed"
      : ["listening", "silence", "idle"].includes(props.phase)
        ? "recording"
        : props.phase === "transcribing"
          ? "transcribing"
          : props.phase === "processing"
            ? "processing"
            : props.phase === "error"
              ? "error"
              : "closed";
  return {
    applicationProfile: props.applicationProfile,
    phase,
    sessionId: pending?.sessionId ?? 0,
    elapsedMs: props.elapsed * 1000,
    level: props.level,
    result: pending?.resultText || pending?.originalText || "",
    error,
    insertionBlocked: pending?.insertionBlocked ?? false,
    copying: props.copying ?? false,
    status:
      props.outcome === "no_speech"
        ? message("dictation.noSpeech")
        : props.phase === "silence" && !pending
          ? `Пауза · ${props.seconds} с`
          : undefined,
    postprocessingOn:
      choice.value.processingEnabled ?? props.preferences.processingEnabled,
    translationOn:
      choice.value.targetLanguage !== null &&
      (choice.value.translationEnabled ??
        props.preferences.processingTranslationEnabled),
    style: choice.value.preset,
    language: choice.value.targetLanguage ?? "en",
    recognitionLanguage:
      choice.value.recognitionLanguage ?? props.preferences.language,
    recognition:
      props.recognition ??
      previewRecognitionCapability(props.preferences.model),
  };
});
const quick = computed(
  () =>
    props.preferences.hotkeyMode === "toggle" &&
    props.preferences.overlayQuickProcessing,
);
function choose(patch: Partial<IndicatorChoice>) {
  const current = indicator.value;
  emit("processingChange", {
    preset: patch.style ?? current.style,
    targetLanguage: patch.language ?? current.language,
    processingEnabled: patch.postprocessingOn ?? current.postprocessingOn,
    translationEnabled: patch.translationOn ?? current.translationOn,
    ...(current.phase === "recording" && current.recognition?.options.length
      ? {
          recognitionLanguage:
            patch.recognitionLanguage ?? current.recognitionLanguage,
        }
      : {}),
  });
}
</script>
<template>
  <div class="overlay-preview-wrap">
    <OverlayIndicator
      class="overlay-preview"
      :class="{ disabled: !preferences.overlayEnabled }"
      :style="{
        zoom: preferences.overlayScale / 100,
        '--fono-overlay-zoom': preferences.overlayScale / 100,
        opacity: preferences.overlayOpacity / 100,
      }"
      :state="indicator"
      :compact="preferences.overlayCompact"
      :hotkey-mode="preferences.hotkeyMode"
      :dictation-trigger="preferences.dictationTrigger"
      :quick="quick"
      :saving="processingSaving"
      :can-finish="interactive && canFinish"
      :can-retry="
        !!pending?.error &&
        pending.processingEnabled &&
        !pending.insertionBlocked
      "
      @accept="emit('finish')"
      @cancel="emit('cancel')"
      @close="emit('cancel')"
      @copy="emit('copy', indicator.result)"
      @choose="choose"
      @retry="
        pending &&
        emit('resolve', {
          sessionId: pending.sessionId,
          action: 'process_preview',
        })
      "
      @panel-change="(open) => emit('helpChange', open)"
    />
  </div>
</template>
