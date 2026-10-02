<script setup lang="ts">
import { computed } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
import { microphones, PreferenceToggle } from "../../preferences";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import StatusDot from "../../../shared/presentation/StatusDot.vue";
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
    panel: QuickPanel;
    value: string;
    health: Health;
    toggle?: ToggleKey;
  }[]
>(() => {
  const p = workspace.state.preferences;
  const model = workspace.state.models.find((m) => m.id === p.model);
  return [
    {
      title: "Микрофон",
      panel: "microphone",
      value: workspace.state.microphoneAvailable
        ? microphones.find((m) => m.value === p.microphone)?.label ||
          p.microphone
        : "Подключите устройство",
      health: workspace.state.microphoneAvailable ? "ready" : "missing",
    },
    {
      title: "Пробуждение",
      panel: "wake",
      value: p.wakePhrase,
      health: p.wakeEnabled ? "ready" : "off",
      toggle: "wakeEnabled",
    },
    {
      title: "Распознавание",
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
      panel: "processing",
      value: p.processingModel,
      health: !p.processingEnabled
        ? "off"
        : workspace.state.aiAvailable
          ? "ready"
          : "error",
      toggle: "processingEnabled",
    },
  ];
});
</script>
<template>
  <section class="voice-tools" aria-label="Основные инструменты">
    <article v-for="tool in tools" :key="tool.panel" class="tool-card">
      <div class="tool-title">
        <strong>{{ tool.title }}</strong
        ><PreferenceToggle
          v-if="tool.toggle"
          :name="tool.toggle"
          :label="tool.title"
          compact
        />
      </div>
      <div class="tool-value" :title="tool.value">{{ tool.value }}</div>
      <div class="tool-bottom">
        <StatusDot :health="tool.health" /><WlButton
          size="xs"
          variant="ghost"
          :aria-label="'Настроить: ' + tool.title"
          @click="ui.openQuick(tool.panel)"
          ><template #icon><AppIcon name="settings" :size="14" /></template
          >Настроить</WlButton
        >
      </div>
    </article>
  </section>
</template>
