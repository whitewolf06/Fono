<script setup lang="ts">
import { computed } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import {
  useWorkspace,
  phaseLabels,
} from "../../../shared/application/workspace";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import VoiceWave from "./VoiceWave.vue";
const workspace = useWorkspace();
const recording = computed(() =>
  ["listening", "silence"].includes(workspace.state.phase),
);
const working = computed(() =>
  ["transcribing", "processing"].includes(workspace.state.phase),
);
</script>
<template>
  <section class="record-area" aria-label="Диктовка">
    <VoiceWave
      :phase="workspace.state.phase"
      :level="workspace.state.audioLevel"
    />
    <div class="record-controls">
      <span class="record-status" role="status"
        >{{ phaseLabels[workspace.state.phase]
        }}<span v-if="recording" class="mono"
          >{{
            Math.floor(workspace.state.elapsed / 60)
              .toString()
              .padStart(2, "0")
          }}:{{
            Math.floor(workspace.state.elapsed % 60)
              .toString()
              .padStart(2, "0")
          }}</span
        ></span
      ><WlButton
        size="sm"
        :variant="recording ? 'soft-danger' : 'soft'"
        :loading="working"
        @click="
          recording ? workspace.dictation.finish() : workspace.dictation.start()
        "
        ><template #icon
          ><AppIcon
            :name="recording ? 'stop' : 'microphone'"
            :size="16" /></template
        >{{
          recording ? "Завершить" : working ? "Обработка" : "Начать запись"
        }}</WlButton
      ><WlButton
        v-if="recording || working"
        size="sm"
        variant="ghost"
        @click="workspace.dictation.cancel()"
        >Отмена</WlButton
      >
    </div>
    <p v-if="workspace.state.error" class="record-error" role="alert">
      {{ workspace.state.error }}
      <RouterLink
        :to="
          workspace.state.microphoneAvailable
            ? '/settings/processing'
            : '/settings/audio'
        "
        >Настройки</RouterLink
      >
    </p>
  </section>
</template>
