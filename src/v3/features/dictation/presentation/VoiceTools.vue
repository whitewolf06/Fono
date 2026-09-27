<script setup lang="ts">
import type { VoiceTools } from "../domain/voice";

defineProps<{ tools: VoiceTools }>();

const items: { key: keyof VoiceTools; title: string }[] = [
  { key: "microphone", title: "Микрофон" },
  { key: "wakeWord", title: "WakeWord" },
  { key: "postProcessing", title: "Постобработка" },
  { key: "recognition", title: "Распознавание" },
];
</script>

<template>
  <div class="v3-tool-strip" aria-label="Основные инструменты">
    <div
      v-for="item in items"
      :key="item.key"
      class="v3-tool"
      :class="{ 'is-off': !tools[item.key].enabled }"
    >
      <span class="v3-tool-heading">
        <span class="v3-tool-dot" aria-hidden="true"></span>
        {{ item.title }}
      </span>
      <span class="v3-tool-value" :title="tools[item.key].value">
        {{ tools[item.key].value }}
      </span>
      <span class="v3-visually-hidden">
        {{ tools[item.key].enabled ? "Включено" : "Выключено" }}
      </span>
    </div>
  </div>
</template>
