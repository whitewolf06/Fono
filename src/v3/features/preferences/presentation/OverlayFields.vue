<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed, reactive, ref, watch } from "vue";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
const workspace = useWorkspace();
const { run } = useFeedback();
import { WlSlider, WlButton } from "@whitelife-core/ui-kit";
import type { Phase, Preferences } from "../../../shared/domain/contracts";
import type {
  PendingDictation,
  PendingDictationRequest,
  OverlayProcessingChoice,
} from "../../../shared/domain/processing";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import PreferenceToggle from "./PreferenceToggle.vue";
import { OverlayPreview } from "../../overlay";
import SelectField from "../../../shared/presentation/SelectField.vue";
import SettingsDisclosure from "../../../shared/presentation/SettingsDisclosure.vue";
const draft = defineModel<Preferences>({ required: true });
const previewChoice = reactive({
  processingMode: draft.value.processingMode,
  processingTranslation: draft.value.processingTranslation,
  processingEnabled: draft.value.processingEnabled,
  processingTranslationEnabled: draft.value.processingTranslationEnabled,
  language: draft.value.language,
});
const previewPreferences = computed(() => ({
  ...draft.value,
  ...previewChoice,
}));
const previewScenario = ref("recording");
const previewFeedback = ref("");
const previewResult = ref<string | null>(null);
let previewRevision = 0;
const sampleText =
  "Проверить новую версию Fono и отправить команде результаты.";
const previewPhase = computed<Phase>(() => {
  const phases: Record<string, Phase> = {
    recording: "listening",
    silence: "silence",
    awaiting_action: "awaiting_action",
    processing: "processing",
    done: "done",
    cancelled: "cancelled",
    error: "error",
  };
  return phases[previewScenario.value] || "listening";
});
const demoPending = computed<PendingDictation | null>(() =>
  previewScenario.value === "awaiting_action"
    ? {
        sessionId: previewRevision,
        phase: "awaiting_action" as const,
        originalText: sampleText,
        resultText: previewResult.value,
        copyOnly: true,
        preset: previewChoice.processingMode,
        targetLanguage:
          !previewChoice.processingTranslationEnabled ||
          previewChoice.processingTranslation === "none"
            ? null
            : previewChoice.processingTranslation,
        processingEnabled: previewChoice.processingEnabled,
        translationEnabled: previewChoice.processingTranslationEnabled,
        source: "hotkey" as const,
        createdAt: "2026-10-04T12:00:00.000Z",
        error: null,
        insertionBlocked: false,
      }
    : null,
);
watch(
  previewScenario,
  () => {
    previewRevision++;
    previewFeedback.value = "";
    previewResult.value = null;
  },
  { flush: "sync" },
);
function resolvePreview(request: PendingDictationRequest) {
  if (demoPending.value?.sessionId !== request.sessionId) return;
  if (request.action === "process_preview") {
    previewResult.value = sampleText;
    previewFeedback.value = uiMessage("ui.196fc69bcb");
    return;
  }
  previewScenario.value = request.action === "cancel" ? "cancelled" : "done";
  previewFeedback.value = uiMessage("ui.604f1fdd67");
}
async function copyPreview(text: string) {
  const revision = previewRevision;
  const copied = await run(
    () => workspace.copy(text),
    uiMessage("ui.5ee87f2703"),
  );
  if (copied && revision === previewRevision) previewScenario.value = "done";
}
function changePreviewProcessing(choice: OverlayProcessingChoice) {
  previewChoice.processingMode = choice.preset;
  previewChoice.processingTranslation = choice.targetLanguage ?? "none";
  if (choice.processingEnabled !== undefined)
    previewChoice.processingEnabled = choice.processingEnabled;
  if (choice.translationEnabled !== undefined)
    previewChoice.processingTranslationEnabled = choice.translationEnabled;
  if (choice.recognitionLanguage !== undefined)
    previewChoice.language = choice.recognitionLanguage;
}
</script>
<template>
  <div class="form-stack">
    <PreferenceToggle
      name="overlayEnabled"
      :label="$t('ui.dbcc59cc37')"
      :description="$t('ui.94e960064b')"
    /><PreferenceToggle
      name="overlayCompact"
      :label="$t('ui.a3a998bfd2')"
      :description="$t('ui.2a4a3dffd8')"
    />
    <label id="overlayScale"
      >{{ $t("ui.a8da764ef3") }} {{ draft.overlayScale }}%<WlSlider
        v-model="draft.overlayScale"
        :min="70"
        :max="130"
        :step="5"
        :aria-label="$t('ui.42d639bf3b')"
    /></label>
    <SelectField
      v-model="draft.overlayPosition"
      :label="$t('ui.8b0130c9c1')"
      :options="[
        {
          value: 'custom',
          get label() {
            return $t('ui.918b03ab75');
          },
        },
        {
          value: 'bottom',
          get label() {
            return $t('ui.2e07b75906');
          },
        },
        {
          value: 'top',
          get label() {
            return $t('ui.732719192d');
          },
        },
      ]"
    />

    <div class="preview-stage" :data-position="draft.overlayPosition">
      <OverlayPreview :preferences="draft" phase="listening" /><small>{{
        $t("ui.d8ec86c062")
      }}</small>
    </div>
    <SettingsDisclosure id="overlay-advanced">
      <PreferenceToggle
        id="overlayQuickProcessing"
        name="overlayQuickProcessing"
        :label="$t('ui.f793997a46')"
        :description="$t('ui.0fab9c0bcf')"
      />
      <label id="overlayOpacity"
        >{{ $t("ui.ea7426e01f") }} {{ draft.overlayOpacity }}%<WlSlider
          v-model="draft.overlayOpacity"
          :min="30"
          :max="100"
          :step="5"
          :aria-label="$t('ui.669005b3e0')"
      /></label>
      <SettingsDisclosure
        id="overlay-preview"
        :title="$t('settings.overlayTest')"
      >
        <SelectField
          v-model="previewScenario"
          :label="$t('ui.80f639d8cf')"
          :options="[
            {
              value: 'recording',
              get label() {
                return $t('ui.1d80e1757a');
              },
            },
            {
              value: 'silence',
              get label() {
                return $t('ui.49e7286558');
              },
            },
            {
              value: 'awaiting_action',
              get label() {
                return $t('ui.20170aee15');
              },
            },
            {
              value: 'processing',
              get label() {
                return $t('ui.7578d9d962');
              },
            },
            {
              value: 'done',
              get label() {
                return $t('ui.ef05d57959');
              },
            },
            {
              value: 'error',
              get label() {
                return $t('ui.bf22dfeeba');
              },
            },
            {
              value: 'cancelled',
              get label() {
                return $t('ui.23a2a9bfcf');
              },
            },
          ]"
          :hint="$t('ui.f8120814d5')"
        />
        <div class="preview-stage" :data-position="draft.overlayPosition">
          <OverlayPreview
            :preferences="previewPreferences"
            :phase="previewPhase"
            :pending="demoPending"
            interactive
            @finish="previewScenario = 'awaiting_action'"
            @cancel="previewScenario = 'cancelled'"
            @resolve="resolvePreview"
            @copy="copyPreview"
            @processing-change="changePreviewProcessing"
          /><small>{{ $t("ui.d8ec86c062") }}</small>
          <p v-if="previewFeedback" class="notice" role="status">
            {{ previewFeedback }}
          </p>
        </div>
        <details class="advanced">
          <summary>{{ $t("ui.f7bece44b8") }}</summary>
          <div class="form-stack overlay-legend">
            <p class="muted">
              <AppIcon
                name="check"
                :size="14"
                class="dictation-action--insert"
              />
              {{ $t("ui.5f833c640b") }}
            </p>
            <p class="muted">
              <AppIcon
                name="keyboard"
                :size="14"
                class="dictation-action--insert"
              />
              {{ $t("ui.df6576f49d") }}
            </p>
            <p class="muted">
              <AppIcon
                name="sparkle"
                :size="14"
                class="dictation-action--process"
              />
              {{ $t("ui.fa65b7dee7") }}
            </p>
            <p class="muted">
              <AppIcon
                name="translate"
                :size="14"
                class="dictation-action--translate"
              />
              {{ $t("ui.f7458b5a7e") }}
            </p>
            <p class="muted">
              <AppIcon name="x" :size="14" class="dictation-action--cancel" />
              {{ $t("ui.4cd6ae4763") }}
            </p>
            <p class="muted">{{ $t("ui.e641b1266d") }}</p>
            <p class="muted">{{ $t("ui.024dc368df") }}</p>
          </div>
        </details>
        <div class="actions">
          <WlButton
            v-if="workspace.settings.previewOverlay"
            size="sm"
            @click="run(() => workspace.settings.previewOverlay!(draft))"
            >{{ $t("ui.9717dd97d1") }}</WlButton
          >
          <WlButton
            size="sm"
            @click="
              workspace.settings.resetOverlay
                ? run(() => workspace.settings.resetOverlay!())
                : (draft.overlayPosition = 'bottom')
            "
            >{{ $t("ui.504e50f3e5") }}</WlButton
          ><RouterLink class="text-link" to="/overlay">{{
            $t("ui.7a06baba55")
          }}</RouterLink>
        </div>
      </SettingsDisclosure>
    </SettingsDisclosure>
  </div>
</template>
