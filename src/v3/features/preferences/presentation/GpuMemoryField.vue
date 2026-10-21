<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed } from "vue";
import type { Preferences } from "../../../shared/domain/contracts";
import { gpuMemoryUsage } from "../../../shared/domain/gpuMemory";
import { useWorkspace } from "../../../shared/application/workspace";
import SelectField from "../../../shared/presentation/SelectField.vue";

const draft = defineModel<Preferences>({ required: true });
const workspace = useWorkspace();
const status = computed(() => workspace.state.gpuMemory);
const usage = computed(() => gpuMemoryUsage(status.value));
const options = [
  {
    value: "resident",
    get label() {
      return uiMessage("ui.22cff93398");
    },
  },
  {
    value: "adaptive",
    get label() {
      return uiMessage("ui.9393fd969c");
    },
  },
];
</script>
<template>
  <div class="form-stack">
    <SelectField
      id="gpu-memory"
      v-model="draft.gpuModelResidency"
      :label="$t('ui.4cfc792253')"
      :options="options"
      :disabled="draft.acceleration === 'cpu'"
      :hint="
        draft.acceleration === 'cpu'
          ? $t('ui.949ade78a7')
          : draft.gpuModelResidency === 'adaptive'
            ? $t('ui.79c5de714b')
            : $t('ui.8d9115887c')
      "
    />
    <p v-if="workspace.native && status" class="notice" aria-live="polite">
      {{ $t("ui.2aa98ec188") }} {{ status.message
      }}<span v-if="usage"> · {{ usage }}</span>
    </p>
    <small v-else class="muted">
      {{
        workspace.state.gpuMemoryError ||
        (workspace.native ? $t("ui.d8cbb438f7") : $t("ui.4af93af552"))
      }}
    </small>
    <small v-if="draft.gpuModelResidency === 'adaptive'" class="muted">
      {{ $t("ui.658df709f9") }}
    </small>
  </div>
</template>
