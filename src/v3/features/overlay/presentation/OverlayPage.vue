<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

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
import SelectionOverlay from "../../selection/presentation/SelectionOverlay.vue";
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
    copyMessage.value = uiMessage("ui.7f9be1d042");
  } catch {
    copyMessage.value = uiMessage("ui.33712388e1");
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
    <PageHeading :title="$t('ui.f4c1856607')" :description="$t('ui.c87f6dd3cc')"
      ><RouterLink class="text-link" to="/settings/overlay">{{
        $t("ui.d6ea193615")
      }}</RouterLink></PageHeading
    >
    <div class="phase-picker" :aria-label="$t('ui.2facbdf1a7')">
      <button
        v-for="value in phases"
        :key="value"
        :class="{ selected: phase === value }"
        @click="select(value)"
      >
        {{ phaseLabels[value] }}
      </button>
      <button @click="showPendingError">{{ $t("ui.57b259cce3") }}</button>
      <button @click="quickRecording">{{ $t("ui.fad33378ff") }}</button>
      <button
        :disabled="!['listening', 'silence'].includes(phase)"
        @click="finish('hotkey')"
      >
        {{ $t("ui.6caa0998f3") }}
      </button>
    </div>
    <div
      v-if="LIVE_DICTATION_ENABLED"
      class="phase-picker"
      :aria-label="$t('ui.e4e922caf1')"
    >
      <button @click="selectLive('recording')">
        {{ $t("ui.4410e08a7a") }}
      </button>
      <button @click="selectLive('paused')">{{ $t("ui.fa516b44f7") }}</button>
      <button @click="selectLive('backlog')">{{ $t("ui.f5f363b0e9") }}</button>
      <button @click="selectLive('error')">{{ $t("ui.c7110b68ed") }}</button>
    </div>
    <div
      class="overlay-desktop"
      :data-position="state.preferences.overlayPosition"
    >
      <div class="desktop-mock-window">
        <span class="desktop-dots">● ● ●</span>
        <h3>{{ $t("ui.2d8c9b1073") }}</h3>
        <p>{{ $t("ui.d10e31c76d") }}</p>
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
          <small>{{ $t("ui.7acc1f2685") }}</small
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
          <small>{{ $t("ui.40db90d722") }}</small
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
      <small class="muted">{{ $t("ui.cff20fc773") }}</small
      ><WlButton size="sm" @click="play">{{ $t("ui.22c1ab8945") }}</WlButton>
    </div>
    <small v-if="copyMessage" class="muted" role="status">{{
      copyMessage
    }}</small>
    <template v-if="!workspace.native">
      <div class="section-header">
        <h3>{{ $t("ui.b97d59e00f") }}</h3>
        <WlButton @click="workspace.selection.demo?.()">{{
          $t("ui.b9d6172309")
        }}</WlButton>
      </div>
      <SelectionOverlay
        :port="workspace.selection"
        :preferences="state.preferences"
        :native="false"
      />
    </template>
  </div>
</template>
