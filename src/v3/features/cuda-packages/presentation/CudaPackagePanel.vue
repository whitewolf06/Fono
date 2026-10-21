<script setup lang="ts">
import {
  message as $t,
  translateAuthored,
} from "../../../shared/domain/messages";

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
    :aria-label="$t('ui.67c88d72c8')"
  >
    <h3>NVIDIA CUDA</h3>
    <p>{{ $t("ui.31a6a5c909") }}</p>
    <p
      v-if="state.nvidiaDetected && state.phase !== 'installed'"
      class="notice"
    >
      {{ $t("ui.962fac93a6") }}
    </p>
    <p aria-live="polite">{{ translateAuthored(state.message || "") }}</p>
    <div v-if="state.phase === 'downloading'" class="form-stack">
      <progress
        :value="progress ?? undefined"
        :max="100"
        :aria-label="$t('ui.3dc37e1b4c')"
      />
      <small class="muted">{{
        progress === null ? $t("ui.db491ef503") : `${progress}%`
      }}</small>
    </div>
    <p v-if="props.unsaved || recording" class="notice">
      {{ $t("ui.c71df745ff") }}
    </p>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    <div class="actions">
      <WlButton
        v-if="!['installed', 'unavailable'].includes(state.phase)"
        size="sm"
        :disabled="busy || actionPending || props.unsaved || recording"
        @click="run(() => port.install())"
        >{{ $t("ui.381ec7e030") }}</WlButton
      >
      <WlButton
        v-if="busy && !['activating', 'cancelling'].includes(state.phase)"
        size="sm"
        @click="run(() => port.cancel())"
        >{{ $t("ui.d60ea58ec4") }}</WlButton
      >
      <WlButton
        size="sm"
        variant="ghost"
        :disabled="busy || actionPending"
        @click="run(() => port.refresh())"
        >{{ $t("ui.d1005e64bf") }}</WlButton
      >
    </div>
    <small v-if="!workspace.native" class="muted">
      {{ $t("ui.bb110cb598") }}
    </small>
  </section>
</template>
<style scoped>
progress {
  width: 100%;
  accent-color: var(--fono-accent);
}
</style>
