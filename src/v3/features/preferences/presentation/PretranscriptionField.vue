<script setup lang="ts">
import { computed } from "vue";
import { useWorkspace } from "../../../shared/application/workspace";
import { message } from "../../../shared/domain/messages";
import {
  captureSettingsLocked,
  pretranscriptionMessage,
} from "../../../shared/domain/pretranscription";
import PreferenceToggle from "./PreferenceToggle.vue";

const workspace = useWorkspace();
const disabled = computed(
  () =>
    captureSettingsLocked(workspace.state.phase) ||
    (workspace.native && !workspace.state.pretranscription?.available),
);
const status = computed(() =>
  pretranscriptionMessage(workspace.state, !!workspace.native),
);
</script>
<template>
  <div id="pretranscriptionEnabled" class="form-stack">
    <PreferenceToggle
      name="pretranscriptionEnabled"
      :label="message('pretranscription.label')"
      :description="message('pretranscription.hint')"
      :disabled="disabled"
    />
    <p class="muted" role="status" aria-live="polite">{{ status }}</p>
  </div>
</template>
