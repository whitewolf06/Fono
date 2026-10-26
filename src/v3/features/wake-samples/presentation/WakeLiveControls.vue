<script setup lang="ts">
import { computed, onMounted } from "vue";
import { WlBadge, WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { message as $t } from "../../../shared/domain/messages";
import { captureSettingsLocked } from "../../../shared/domain/pretranscription";
import { liraActive } from "../domain/samples";
import ErrorHint from "../../../shared/presentation/ErrorHint.vue";
withDefaults(defineProps<{ showError?: boolean }>(), { showError: true });
const workspace = useWorkspace();
const port = workspace.wakeSamples;
const state = port.state;
const liveActive = computed(() => liraActive(state.live));
const blocked = computed(
  () =>
    state.livePending ||
    state.phase !== "idle" ||
    state.testingId !== null ||
    captureSettingsLocked(workspace.state.phase) ||
    workspace.state.triggerCaptureActive,
);
onMounted(() => void port.refreshLive());
</script>
<template>
  <section
    v-if="state.live.available"
    id="wake-live"
    class="form-stack"
    :aria-label="$t('wakeLive.title')"
  >
    <h3>
      {{ $t("wakeLive.title") }}
      <WlBadge :value="$t('wakeLive.beta')" variant="gray" />
    </h3>
    <p class="notice">{{ $t("wakeLive.hint") }}</p>
    <p class="muted">{{ $t("wakeLive.betaHint") }}</p>
    <p v-if="state.live.phase !== 'error'" role="status" aria-live="polite">
      {{ $t(`wakeLive.${state.live.phase}`) }}
    </p>
    <ErrorHint
      v-if="state.live.message || (showError && state.error)"
      :detail="state.live.message || state.error"
    />
    <div class="actions">
      <WlButton
        v-if="!liveActive"
        variant="primary"
        :disabled="blocked"
        @click="port.enableLive()"
        >{{ $t("wakeLive.enable") }}</WlButton
      >
      <WlButton
        v-else
        :disabled="state.livePending || state.live.phase === 'stopping'"
        @click="port.disableLive()"
        >{{ $t("wakeLive.disable") }}</WlButton
      >
    </div>
    <p v-if="liveActive" class="muted">{{ $t("wakeLive.samplesPaused") }}</p>
  </section>
</template>
