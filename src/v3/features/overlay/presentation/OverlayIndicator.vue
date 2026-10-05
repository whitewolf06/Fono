<script setup lang="ts">
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
const props = withDefaults(
  defineProps<{
    state: IndicatorState;
    compact?: boolean;
    quick?: boolean;
    saving?: boolean;
    canFinish?: boolean;
    canRetry?: boolean;
    hotkeyMode?: "hold" | "toggle";
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
const panel = ref<"help" | "processing" | "translation" | null>(null);
const panelId = useId();
const labels: Record<IndicatorPhase, string> = {
  recording: "Слушаю вас",
  transcribing: "Распознаю речь",
  processing: "Обрабатываю текст",
  ready: "Текст готов",
  error: "Нужна ваша помощь",
  closed: "Индикатор закрыт",
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
          title="Что означают кнопки"
          aria-label="Что означают кнопки"
          :aria-expanded="panel === 'help'"
          :aria-controls="panelId"
          @click="configure('help')"
        >
          <AppIcon name="info" :size="13" />
        </button>
      </div>
      <div class="overlay-indicator-summary" aria-live="polite">
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
        <OverlayIndicatorToggles
          v-if="quick"
          :choice="state"
          :disabled="!recording || saving"
          :panel="panel"
          :panel-id="panelId"
          @choose="(patch) => emit('choose', patch)"
          @configure="configure"
        />
        <div
          class="overlay-indicator-actions"
          role="group"
          aria-label="Действия с диктовкой"
        >
          <button
            type="button"
            class="overlay-indicator-action cancel"
            :title="ready ? 'Закрыть без копирования' : 'Отменить запись'"
            :aria-label="ready ? 'Закрыть без копирования' : 'Отменить запись'"
            @click="ready ? emit('close') : emit('cancel')"
          >
            <AppIcon name="x" :size="20" />
          </button>
          <button
            v-if="ready && canRetry"
            type="button"
            class="overlay-indicator-action processing"
            title="Повторить обработку без вставки"
            aria-label="Повторить обработку без вставки"
            :disabled="saving || state.copying"
            @click="emit('retry')"
          >
            <AppIcon name="undo" :size="18" />
          </button>
          <button
            v-if="ready"
            type="button"
            class="overlay-indicator-action copy"
            title="Скопировать текст и закрыть"
            aria-label="Скопировать текст и закрыть"
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
            title="Завершить запись без вставки"
            aria-label="Завершить запись без вставки"
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
      <OverlayIndicatorHelp v-if="panel === 'help'" :hotkey-mode="hotkeyMode" />
      <template v-else-if="panel === 'processing'">
        <strong>Стиль постобработки</strong>
        <div
          class="overlay-indicator-options"
          role="group"
          aria-label="Стиль постобработки"
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
        <small>Применится при завершении текущей записи.</small>
      </template>
      <template v-else>
        <strong>Язык перевода</strong>
        <div
          class="overlay-indicator-options"
          role="group"
          aria-label="Язык перевода"
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
        <small
          >Перевод выполняется после постобработки. Переключатель позволяет
          быстро отключить его.</small
        >
      </template>
    </div>
  </div>
</template>
