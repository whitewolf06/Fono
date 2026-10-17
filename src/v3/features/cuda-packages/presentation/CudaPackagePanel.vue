<script setup lang="ts">
import { computed } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import {
  cudaPackageBusy,
  cudaPackageProgress,
} from "../../../shared/domain/cudaPackages";
const props = defineProps<{ unsaved: boolean }>();
const workspace = useWorkspace();
const port = workspace.cudaPackages;
const state = port.state;
const { run, error, busy: actionPending } = useFeedback();
const busy = computed(() => cudaPackageBusy(state.phase));
const progress = computed(() => cudaPackageProgress(state));
const recording = computed(
  () =>
    !!workspace.state.pendingDictation ||
    [
      "listening",
      "silence",
      "transcribing",
      "processing",
      "awaiting_action",
    ].includes(workspace.state.phase),
);
</script>
<template>
  <section
    id="cuda-package"
    class="form-stack"
    aria-label="Ускорение NVIDIA CUDA"
  >
    <h3>NVIDIA CUDA</h3>
    <p>
      CUDA загружается отдельно. До установки в режиме «Авто» работает Vulkan
      или процессор.
    </p>
    <p
      v-if="state.nvidiaDetected && state.phase !== 'installed'"
      class="notice"
    >
      Найдена видеокарта NVIDIA. Можно установить пакет CUDA для локального
      распознавания.
    </p>
    <p aria-live="polite">{{ state.message }}</p>
    <div v-if="state.phase === 'downloading'" class="form-stack">
      <progress
        :value="progress ?? undefined"
        :max="100"
        aria-label="Загрузка пакета CUDA"
      />
      <small class="muted">{{
        progress === null ? "Загрузка…" : `${progress}%`
      }}</small>
    </div>
    <p v-if="props.unsaved || recording" class="notice">
      Перед установкой завершите диктовку и сохраните настройки.
    </p>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    <div class="actions">
      <WlButton
        v-if="!['installed', 'unavailable'].includes(state.phase)"
        size="sm"
        :disabled="busy || actionPending || props.unsaved || recording"
        @click="run(() => port.install())"
        >Скачать и установить CUDA</WlButton
      >
      <WlButton
        v-if="busy && !['activating', 'cancelling'].includes(state.phase)"
        size="sm"
        @click="run(() => port.cancel())"
        >Отменить загрузку</WlButton
      >
      <WlButton
        size="sm"
        variant="ghost"
        :disabled="busy || actionPending"
        @click="run(() => port.refresh())"
        >Обновить состояние</WlButton
      >
    </div>
    <small v-if="!workspace.native" class="muted">
      Браузерный макет: демонстрация без загрузки файлов и доступа к видеокарте.
    </small>
  </section>
</template>
<style scoped>
progress {
  width: 100%;
  accent-color: var(--fono-accent);
}
</style>
