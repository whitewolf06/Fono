<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed, ref, useId, watch } from "vue";
import type {
  IndicatorState,
  IndicatorChoice,
  IndicatorPhase,
} from "../domain/indicator";
import type { Phase } from "../../../shared/domain/contracts";
import {
  presetOptions,
  translationOptions,
} from "../../../shared/domain/processing";
import { VoiceWave } from "../../dictation";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import { dictationNotice } from "../../../shared/domain/dictationNotice";
import DictationNotice from "../../../shared/presentation/DictationNotice.vue";
import OverlayIndicatorToggles from "./OverlayIndicatorToggles.vue";
import OverlayIndicatorHelp from "./OverlayIndicatorHelp.vue";
import { recognitionOptions } from "../../../shared/domain/recognition";
const props = withDefaults(
  defineProps<{
    state: IndicatorState;
    compact?: boolean;
    quick?: boolean;
    saving?: boolean;
    canFinish?: boolean;
    canRetry?: boolean;
    hotkeyMode?: "hold" | "toggle";
    dictationTrigger?: string;
  }>(),
  { quick: true, saving: false, canFinish: true },
);
const emit = defineEmits<{
  accept: [];
  cancel: [];
  copy: [];
  close: [];
  choose: [patch: Partial<IndicatorChoice>];
  panelChange: [open: boolean];
  retry: [];
}>();
const panel = ref<"help" | "processing" | "translation" | "recognition" | null>(
  null,
);
const panelId = useId();
const labels: Record<IndicatorPhase, string> = {
  get recording() {
    return uiMessage("ui.9d58522b4a");
  },
  get transcribing() {
    return uiMessage("ui.3ae9f114b8");
  },
  get processing() {
    return uiMessage("ui.7fd3adbb0b");
  },
  get ready() {
    return uiMessage("ui.617d6b596d");
  },
  get error() {
    return uiMessage("ui.933abea3ee");
  },
  get closed() {
    return uiMessage("ui.f8762efac1");
  },
};
const wavePhase: Record<IndicatorPhase, Phase> = {
  recording: "listening",
  transcribing: "transcribing",
  processing: "processing",
  ready: "done",
  error: "error",
  closed: "idle",
};
const ready = computed(
  () => !!props.state.result && ["ready", "error"].includes(props.state.phase),
);
const recording = computed(() => props.state.phase === "recording");
const notice = computed(() =>
  dictationNotice(props.state.error, {
    insertionBlocked: props.state.insertionBlocked,
    hasText: !!props.state.result,
  }),
);
const status = computed(
  () => notice.value?.title || props.state.status || labels[props.state.phase],
);
const clock = computed(() => {
  const seconds = Math.floor(props.state.elapsedMs / 1000);
  return `${Math.floor(seconds / 60)
    .toString()
    .padStart(2, "0")}:${(seconds % 60).toString().padStart(2, "0")}`;
});
function configure(value: typeof panel.value) {
  panel.value = panel.value === value ? null : value;
}
watch(panel, (value) => emit("panelChange", value !== null));
watch(
  () => props.state.sessionId,
  () => {
    panel.value = null;
  },
);
watch(recording, (value) => {
  if (!value && panel.value !== "help") panel.value = null;
});
</script>
<template>
  <div
    class="overlay-indicator"
    :class="{ compact, 'is-ready': ready, 'is-recording': recording }"
    :data-phase="state.phase"
    :style="{ '--indicator-level': state.level }"
    @keydown.esc.stop="panel = null"
  >
    <div class="overlay-indicator-body">
      <div class="overlay-indicator-brand">
        <img src="/fono-icon.png" alt="" aria-hidden="true" />
        <button
          type="button"
          class="overlay-indicator-info"
          :title="$t('ui.f7bece44b8')"
          :aria-label="$t('ui.f7bece44b8')"
          :aria-expanded="panel === 'help'"
          :aria-controls="panelId"
          @click="configure('help')"
        >
          <AppIcon name="info" :size="13" />
        </button>
      </div>
      <div class="overlay-indicator-summary" aria-live="polite">
        <small
          v-if="state.applicationProfile"
          class="overlay-indicator-profile"
          :title="state.applicationProfile"
          >{{ $t("ui.db7ceb8f20") }} {{ state.applicationProfile }}</small
        >
        <div class="overlay-indicator-status">
          <strong :title="status">{{ status }}</strong
          ><small>{{ clock }}</small>
        </div>
        <p v-if="ready" class="overlay-indicator-result" :title="state.result">
          {{ state.result }}
        </p>
        <VoiceWave
          v-else-if="!notice"
          :phase="wavePhase[state.phase]"
          :level="state.level"
        />
      </div>
      <div class="overlay-indicator-right">
        <div v-if="quick" class="overlay-indicator-settings">
          <button
            v-if="state.recognition?.options.length"
            type="button"
            class="overlay-indicator-recognition"
            :aria-label="$t('ui.629e35250c')"
            :title="
              $t('ui.c8e1cf10d7', {
                v0: String(
                  recognitionOptions.find(
                    (item) => item.value === state.recognitionLanguage,
                  )?.label ?? state.recognitionLanguage,
                ),
              })
            "
            :aria-expanded="panel === 'recognition'"
            :aria-controls="panelId"
            :disabled="!recording || saving"
            @click="configure('recognition')"
          >
            {{
              recognitionOptions.find(
                (item) => item.value === state.recognitionLanguage,
              )?.label ?? state.recognitionLanguage
            }}
          </button>
          <small
            v-else-if="state.recognition?.fixedLanguage"
            class="overlay-indicator-language-hint"
            :title="
              state.recognition.fixedLanguage === 'ru'
                ? $t('ui.613f8e8ff8')
                : $t('ui.f54a3a8c08')
            "
            >{{
              state.recognition.fixedLanguage === "ru"
                ? $t("ui.0c8fc9ca83")
                : $t("ui.c18f3a871d")
            }}</small
          >
          <OverlayIndicatorToggles
            :choice="state"
            :disabled="!recording || saving"
            :panel="panel"
            :panel-id="panelId"
            @choose="(patch) => emit('choose', patch)"
            @configure="configure"
          />
        </div>
        <div
          class="overlay-indicator-actions"
          role="group"
          :aria-label="$t('ui.48124ad766')"
        >
          <button
            type="button"
            class="overlay-indicator-action cancel"
            :title="ready ? $t('ui.965e87fa29') : $t('ui.d5e3f6c438')"
            :aria-label="ready ? $t('ui.965e87fa29') : $t('ui.d5e3f6c438')"
            @click="ready ? emit('close') : emit('cancel')"
          >
            <AppIcon name="x" :size="20" />
          </button>
          <button
            v-if="ready && canRetry"
            type="button"
            class="overlay-indicator-action processing"
            :title="$t('ui.c228bcbb77')"
            :aria-label="$t('ui.c228bcbb77')"
            :disabled="saving || state.copying"
            @click="emit('retry')"
          >
            <AppIcon name="undo" :size="18" />
          </button>
          <button
            v-if="ready"
            type="button"
            class="overlay-indicator-action copy"
            :title="$t('ui.b5ebb76555')"
            :aria-label="$t('ui.b5ebb76555')"
            :disabled="state.copying"
            :aria-busy="state.copying"
            @click="emit('copy')"
          >
            <AppIcon name="copy" :size="19" />
          </button>
          <button
            v-else
            type="button"
            class="overlay-indicator-action accept"
            :title="$t('ui.9f8d67e76a')"
            :aria-label="$t('ui.9f8d67e76a')"
            :disabled="!recording || saving || !canFinish"
            @click="emit('accept')"
          >
            <AppIcon name="check" :size="22" />
          </button>
        </div>
      </div>
    </div>
    <DictationNotice v-if="notice" :notice="notice" :show-title="false" />
    <div v-if="panel" :id="panelId" class="overlay-indicator-panel">
      <OverlayIndicatorHelp
        v-if="panel === 'help'"
        :hotkey-mode="hotkeyMode"
        :dictation-trigger="dictationTrigger"
      />
      <template v-else-if="panel === 'processing'">
        <strong>{{ $t("ui.8885fed5dd") }}</strong>
        <div
          class="overlay-indicator-options"
          role="group"
          :aria-label="$t('ui.8885fed5dd')"
        >
          <button
            v-for="option in presetOptions"
            :key="option.value"
            type="button"
            :aria-pressed="state.style === option.value"
            :disabled="saving"
            @click="emit('choose', { style: option.value })"
          >
            {{ option.label }}
          </button>
        </div>
        <small>{{ $t("ui.54de9b2dce") }}</small>
      </template>
      <template v-else-if="panel === 'recognition'">
        <strong>{{ $t("ui.ede3bcb782") }}</strong>
        <div
          class="overlay-indicator-options"
          role="group"
          :aria-label="$t('ui.ede3bcb782')"
        >
          <button
            v-for="option in recognitionOptions.filter((item) =>
              state.recognition?.options.includes(item.value),
            )"
            :key="option.value"
            type="button"
            :aria-pressed="state.recognitionLanguage === option.value"
            :disabled="!recording || saving"
            @click="emit('choose', { recognitionLanguage: option.value })"
          >
            {{ option.label }}
          </button>
        </div>
        <small>{{ $t("ui.eb14d29ad8") }}</small>
      </template>
      <template v-else>
        <strong>{{ $t("ui.16b43a4ab6") }}</strong>
        <div
          class="overlay-indicator-options"
          role="group"
          :aria-label="$t('ui.16b43a4ab6')"
        >
          <button
            v-for="option in translationOptions.filter(
              (item) => item.value !== 'none',
            )"
            :key="option.value"
            type="button"
            :aria-pressed="state.language === option.value"
            :disabled="saving"
            @click="
              emit('choose', {
                language: option.value as IndicatorChoice['language'],
              })
            "
          >
            {{ option.label }}
          </button>
        </div>
        <small>{{ $t("ui.d8f317f933") }}</small>
      </template>
    </div>
  </div>
</template>
