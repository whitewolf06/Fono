<script setup lang="ts">
import { WlButton } from "@whitelife-core/ui-kit";
import type { Phase } from "../../../shared/domain/contracts";
import {
  useWorkspace,
  phaseLabels,
} from "../../../shared/application/workspace";
import { useOverlayDemo } from "../application/useOverlayDemo";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import OverlayPreview from "./OverlayPreview.vue";
const { state } = useWorkspace();
const { phase, seconds, level, select, play } = useOverlayDemo();
const phases: Phase[] = [
  "idle",
  "listening",
  "silence",
  "transcribing",
  "processing",
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
    </div>
    <div
      class="overlay-desktop"
      :data-position="state.preferences.overlayPosition"
    >
      <div class="desktop-mock-window">
        <span class="desktop-dots">● ● ●</span>
        <h3>Место для ваших мыслей</h3>
        <p>Индикатор остаётся рядом, пока вы работаете.</p>
        <div class="mock-text-line" />
        <div class="mock-text-line short" />
      </div>
      <div class="overlay-variants">
        <section>
          <small>Подробный</small
          ><OverlayPreview
            :preferences="{ ...state.preferences, overlayCompact: false }"
            :phase="phase"
            :level="level"
            :seconds="seconds"
            interactive
            @finish="select('transcribing')"
            @cancel="select('cancelled')"
          />
        </section>
        <section>
          <small>Компактный</small
          ><OverlayPreview
            :preferences="{ ...state.preferences, overlayCompact: true }"
            :phase="phase"
            :level="level"
            :seconds="seconds"
            interactive
            @finish="select('transcribing')"
            @cancel="select('cancelled')"
          />
        </section>
      </div>
    </div>
    <div class="section-header">
      <small class="muted"
        >Масштаб и прозрачность берутся из настроек. Это не настоящее окно
        Tauri.</small
      ><WlButton size="sm" @click="play">Проиграть сценарий</WlButton>
    </div>
  </div>
</template>
