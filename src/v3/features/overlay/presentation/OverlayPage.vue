<script setup lang="ts">
import { ref } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import type { Phase } from "../../../shared/domain/contracts";
import {
  useWorkspace,
  phaseLabels,
} from "../../../shared/application/workspace";
import { useOverlayDemo } from "../application/useOverlayDemo";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import OverlayPreview from "./OverlayPreview.vue";
import { LIVE_DICTATION_ENABLED } from "../../../shared/domain/dictationMode";
const workspace = useWorkspace();
const { state } = workspace;
const demoPreferences = ref({ ...state.preferences });
const copyMessage = ref("");
const {
  phase,
  seconds,
  level,
  live,
  pending,
  resultText,
  select,
  selectLive,
  resumeLive,
  finish,
  cancel,
  play,
  resolve,
  showPendingError,
  chooseProcessing,
} = useOverlayDemo(() => demoPreferences.value);
function quickRecording() {
  demoPreferences.value.hotkeyMode = "toggle";
  demoPreferences.value.processingEnabled = true;
  demoPreferences.value.overlayQuickProcessing = true;
  select("listening");
}
async function copy(text: string) {
  const sessionId = pending.value?.sessionId;
  copyMessage.value = "";
  try {
    await workspace.copy(text);
    if (sessionId && pending.value?.sessionId === sessionId)
      await resolve({ sessionId, action: "complete" });
    copyMessage.value = "Демонстрационный текст скопирован.";
  } catch {
    copyMessage.value =
      "Не удалось скопировать. Выделите текст и скопируйте вручную.";
  }
}
const phases: Phase[] = [
  "idle",
  "listening",
  "silence",
  "transcribing",
  "processing",
  "awaiting_action",
  "error",
  "done",
  "cancelled",
];
</script>
<template>
  <div class="page">
    <PageHeading
      title="Индикатор записи"
      description="Браузерный предпросмотр плавающего окна."
      ><RouterLink class="text-link" to="/settings/overlay"
        >Настройки внешнего вида</RouterLink
      ></PageHeading
    >
    <div class="phase-picker" aria-label="Состояние индикатора">
      <button
        v-for="value in phases"
        :key="value"
        :class="{ selected: phase === value }"
        @click="select(value)"
      >
        {{ phaseLabels[value] }}
      </button>
      <button @click="showPendingError">Ошибка обработки · повторить</button>
      <button @click="quickRecording">Запись · быстрые настройки</button>
      <button
        :disabled="!['listening', 'silence'].includes(phase)"
        @click="finish('hotkey')"
      >
        Завершить сочетанием · демо
      </button>
    </div>
    <div
      v-if="LIVE_DICTATION_ENABLED"
      class="phase-picker"
      aria-label="Состояние живой диктовки"
    >
      <button @click="selectLive('recording')">Живая · запись</button>
      <button @click="selectLive('paused')">Поле изменилось</button>
      <button @click="selectLive('backlog')">Распознавание отстаёт</button>
      <button @click="selectLive('error')">Ошибка вставки</button>
    </div>
    <div
      class="overlay-desktop"
      :data-position="state.preferences.overlayPosition"
    >
      <div class="desktop-mock-window">
        <span class="desktop-dots">● ● ●</span>
        <h3>Место для ваших мыслей</h3>
        <p>Индикатор остаётся рядом, пока вы работаете.</p>
        <p v-if="resultText" class="overlay-demo-result" role="status">
          {{ resultText }}
        </p>
        <template v-else>
          <div class="mock-text-line" />
          <div class="mock-text-line short" />
        </template>
      </div>
      <div class="overlay-variants">
        <section>
          <small>Подробный</small
          ><OverlayPreview
            :preferences="{ ...demoPreferences, overlayCompact: false }"
            :phase="phase"
            :level="level"
            :seconds="seconds"
            :live="live"
            :pending="pending"
            interactive
            @finish="finish('button')"
            @cancel="cancel"
            @resume="resumeLive"
            @resolve="resolve"
            @copy="copy"
            @processing-change="chooseProcessing"
          />
        </section>
        <section>
          <small>Компактный</small
          ><OverlayPreview
            :preferences="{ ...demoPreferences, overlayCompact: true }"
            :phase="phase"
            :level="level"
            :seconds="seconds"
            :live="live"
            :pending="pending"
            interactive
            @finish="finish('button')"
            @cancel="cancel"
            @resume="resumeLive"
            @resolve="resolve"
            @copy="copy"
            @processing-change="chooseProcessing"
          />
        </section>
      </div>
    </div>
    <div class="section-header">
      <small class="muted"
        >Масштаб и прозрачность берутся из настроек. Это не настоящее окно
        Tauri. Действия изменяют только этот предпросмотр.</small
      ><WlButton size="sm" @click="play">Проиграть сценарий</WlButton>
    </div>
    <small v-if="copyMessage" class="muted" role="status">{{
      copyMessage
    }}</small>
  </div>
</template>
