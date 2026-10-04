<script setup lang="ts">
import { computed } from "vue";
import { WlButton, type WlIconName } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
import { microphones, PreferenceToggle } from "../../preferences";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import StatusDot from "../../../shared/presentation/StatusDot.vue";
import {
  WAKE_WORD_AVAILABLE,
  WAKE_WORD_UNAVAILABLE,
} from "../../../shared/domain/wakeAvailability";
import type {
  Health,
  QuickPanel,
  ToggleKey,
} from "../../../shared/domain/contracts";
const workspace = useWorkspace();
const ui = useInteraction();
const tools = computed<
  {
    title: string;
    icon: WlIconName;
    panel: QuickPanel;
    value: string;
    health: Health;
    toggle?: ToggleKey;
  }[]
>(() => {
  const p = workspace.state.preferences;
  const model = workspace.state.models.find((m) => m.id === p.model);
  const translating =
    p.processingTranslationEnabled && p.processingTranslation !== "none";
  const passthrough = p.processingMode === "raw" && !translating;
  return [
    {
      title: "Микрофон",
      icon: "microphone",
      panel: "microphone",
      value: workspace.state.microphoneAvailable
        ? (workspace.state.devices || microphones).find(
            (m) => m.value === p.microphone,
          )?.label || p.microphone
        : "Подключите устройство",
      health: workspace.state.microphoneAvailable ? "ready" : "missing",
    },
    {
      title: "Пробуждение",
      icon: "lightning",
      panel: "wake",
      value: p.wakePhrase,
      health:
        !WAKE_WORD_AVAILABLE || !p.wakeEnabled
          ? "off"
          : ["loading", "initializing"].includes(
                workspace.state.wakeStatus || "",
              )
            ? "loading"
            : workspace.state.wakeStatus === "missing_model" ||
                workspace.state.wakeSetup?.verified === false
              ? "missing"
              : workspace.state.wakeStatus?.startsWith("error")
                ? "error"
                : "ready",
      toggle: "wakeEnabled",
    },
    {
      title: "Распознавание",
      icon: "message",
      panel: "recognition",
      value:
        model?.status === "installed"
          ? model.name
          : model?.status === "downloading"
            ? "Загрузка · " + model.progress + "%"
            : "Установите модель",
      health:
        model?.status === "installed"
          ? "ready"
          : model?.status === "downloading"
            ? "loading"
            : "missing",
    },
    {
      title: "Обработка текста",
      icon: "sparkle",
      panel: "processing",
      value:
        p.dictationMode === "live"
          ? "Без ИИ в живом режиме"
          : p.processingMode === "raw"
            ? translating
              ? `Только перевод · ${p.processingTranslation.toUpperCase()}`
              : "Без изменений · без ИИ"
            : p.processingModel || "Выберите модель",
      health:
        p.dictationMode === "live" || !p.processingEnabled
          ? "off"
          : passthrough
            ? "ready"
            : workspace.native && !workspace.state.aiChecked
              ? p.profile && p.processingModel
                ? "ready"
                : "missing"
              : workspace.state.aiAvailable
                ? "ready"
                : "error",
      toggle: p.dictationMode === "live" ? undefined : "processingEnabled",
    },
  ];
});
</script>
<template>
  <section class="voice-tools" aria-label="Основные инструменты">
    <article v-for="tool in tools" :key="tool.panel" class="tool-card">
      <div class="tool-title">
        <div class="tool-heading">
          <AppIcon class="tool-icon" :name="tool.icon" />
          <strong>{{ tool.title }}</strong>
        </div>
        <PreferenceToggle
          v-if="tool.toggle"
          :name="tool.toggle"
          :label="tool.title"
          compact
        />
      </div>
      <div class="tool-value" :title="tool.value">{{ tool.value }}</div>
      <div class="tool-bottom">
        <StatusDot
          :health="tool.health"
          :label="
            tool.panel === 'wake' && !WAKE_WORD_AVAILABLE
              ? 'Временно недоступно'
              : undefined
          "
        /><WlButton
          size="xs"
          variant="ghost"
          :aria-label="'Настроить: ' + tool.title"
          :disabled="tool.panel === 'wake' && !WAKE_WORD_AVAILABLE"
          :title="
            tool.panel === 'wake' && !WAKE_WORD_AVAILABLE
              ? WAKE_WORD_UNAVAILABLE
              : undefined
          "
          @click="ui.openQuick(tool.panel)"
          ><template #icon><AppIcon name="settings" :size="14" /></template
          >{{
            tool.panel === "wake" && !WAKE_WORD_AVAILABLE
              ? "Скоро"
              : "Настроить"
          }}</WlButton
        >
      </div>
    </article>
  </section>
</template>
