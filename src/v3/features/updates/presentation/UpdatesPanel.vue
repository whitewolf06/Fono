<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
  translateAuthored,
} from "../../../shared/domain/messages";

import { computed, nextTick } from "vue";
import { WlButton, WlSwitch } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
import { useFeedback } from "../../../shared/application/feedback";
import { updateBusy, downloadPercent } from "../../../shared/domain/updates";
const props = defineProps<{ unsaved: boolean }>();
const workspace = useWorkspace();
const ui = useInteraction();
const { run, error, busy: actionPending } = useFeedback();
const port = workspace.updates;
const status = port.state;
const busy = computed(() => updateBusy(status.phase));
let checksInput: HTMLInputElement | undefined;
let restoreChecksFocus = false;
function rememberChecksInput(event: FocusEvent) {
  checksInput = event.currentTarget as HTMLInputElement;
}
function syncChecksInput(event: Event) {
  // A refused controlled change leaves Vue's value unchanged; restore the
  // native checkbox without remounting the keyboard focus target.
  checksInput = event.currentTarget as HTMLInputElement;
  restoreChecksFocus ||=
    checksInput.ownerDocument.activeElement === checksInput;
  checksInput.checked = status.checksEnabled;
}
async function changeChecksEnabled(value: boolean) {
  if (busy.value || actionPending.value || status.phase === "not_configured")
    return;
  restoreChecksFocus =
    !!checksInput && checksInput.ownerDocument.activeElement === checksInput;
  try {
    await run(() => port.setChecksEnabled(value));
  } finally {
    await nextTick();
    const document = checksInput?.ownerDocument;
    if (
      restoreChecksFocus &&
      checksInput?.isConnected &&
      !checksInput.disabled &&
      document?.activeElement === document?.body
    )
      checksInput.focus({ preventScroll: true });
    restoreChecksFocus = false;
  }
}
const progress = computed(() => downloadPercent(status));
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
async function install() {
  if (props.unsaved || recording.value) return;
  const approved = await ui.confirm({
    get title() {
      return uiMessage("ui.f47263c4ca", { v0: String(status.nextVersion) });
    },
    get text() {
      return uiMessage("ui.13672a4371");
    },
    get accept() {
      return uiMessage("ui.be83518bd3");
    },
  });
  if (approved) await run(() => port.install());
}
</script>
<template>
  <section
    id="updates"
    class="form-stack update-settings"
    :aria-label="$t('ui.406ee0f68c')"
  >
    <div class="section-header">
      <h3>{{ $t("ui.455471db0c") }}</h3>
      <small class="muted"
        >{{ $t("ui.0b29a3640f") }} {{ status.currentVersion }}</small
      >
    </div>
    <p aria-live="polite">{{ translateAuthored(status.message) }}</p>
    <p
      v-if="
        status.nextVersion &&
        ['available', 'downloading'].includes(status.phase)
      "
      class="notice"
    >
      {{ $t("ui.0cc229f3cb") }} <strong>{{ status.nextVersion }}</strong>
    </p>
    <div class="toggle-row">
      <span
        ><strong>{{ $t("ui.8db9c5bc9c") }}</strong
        ><small class="muted">{{ $t("ui.d92b070199") }}</small></span
      >
      <WlSwitch
        id="update-checks"
        :aria-label="$t('ui.2e33b6d804')"
        :model-value="status.checksEnabled"
        :disabled="busy || actionPending || status.phase === 'not_configured'"
        @update:model-value="changeChecksEnabled"
        @change="syncChecksInput"
        @focus="rememberChecksInput"
      />
    </div>
    <div v-if="status.phase === 'downloading'" class="form-stack">
      <progress
        :value="progress ?? undefined"
        :max="100"
        :aria-label="$t('ui.b5222cb6c7')"
      />
      <small class="muted">{{
        progress === null
          ? $t("ui.db491ef503")
          : $t("ui.1b253a664e", { v0: String(progress) })
      }}</small>
    </div>
    <p v-if="props.unsaved || recording" class="notice">
      {{ $t("ui.c71df745ff") }}
    </p>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    <div class="actions">
      <WlButton
        size="sm"
        :loading="status.phase === 'checking'"
        :disabled="busy || actionPending || status.phase === 'not_configured'"
        @click="run(() => port.check())"
        >{{ $t("ui.93b08f7b14") }}</WlButton
      >
      <WlButton
        v-if="status.phase === 'available'"
        size="sm"
        variant="primary"
        :disabled="props.unsaved || recording || actionPending"
        @click="install"
        >{{ $t("ui.be83518bd3") }}</WlButton
      >
      <WlButton
        v-if="status.phase === 'downloading'"
        size="sm"
        @click="run(() => port.cancel())"
        >{{ $t("ui.d60ea58ec4") }}</WlButton
      >
    </div>
    <small v-if="!workspace.native" class="muted">{{
      $t("ui.7dd191c085")
    }}</small>
  </section>
</template>
<style scoped>
.update-settings {
  border-top: 1px solid var(--fono-border);
  padding-top: var(--fono-space-4);
}
.toggle-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--fono-space-3);
}
.toggle-row small {
  display: block;
  margin-top: var(--fono-space-1);
}
progress {
  width: 100%;
  accent-color: var(--fono-accent);
}
</style>
