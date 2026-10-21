<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

import { computed, reactive } from "vue";
import { WlDialog, WlInput, WlField, WlButton } from "@whitelife-core/ui-kit";
import type { LaunchApp } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFormGuard } from "../../../shared/application/formGuard";
import { useFeedback } from "../../../shared/application/feedback";
const props = defineProps<{ application: LaunchApp }>();
const emit = defineEmits<{ close: [] }>();
const form = reactive({ ...props.application });
const workspace = useWorkspace();

const { run, error } = useFeedback();
const canDiscard = useFormGuard(
  () => JSON.stringify(form) !== JSON.stringify(props.application),
  () => emit("close"),
);
async function close() {
  if (await canDiscard()) emit("close");
}
const visible = computed({
  get: () => true,
  set: () => {
    void close();
  },
});
</script>
<template>
  <WlDialog
    v-model:visible="visible"
    :header="application.id ? $t('ui.9ef8f3779d') : $t('ui.deafef9756')"
    width="500px"
    ><div class="form-stack">
      <WlField v-slot="field" :label="$t('ui.0918b4ba92')"
        ><WlInput
          v-bind="field"
          v-model="form.name"
          :placeholder="$t('ui.1e01918be9')" /></WlField
      ><WlField v-slot="field" :label="$t('ui.d104467643')"
        ><WlInput
          v-bind="field"
          v-model="form.phrase"
          :placeholder="$t('ui.3d97f44d89')" /></WlField
      ><WlField v-slot="field" :label="$t('ui.855308782e')"
        ><WlInput v-bind="field" v-model="form.path" placeholder="notepad.exe"
      /></WlField>
      <p class="muted">{{ $t("ui.81224e0e15") }}</p>
      <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    </div>
    <template #footer
      ><WlButton @click="close">{{ $t("ui.8fbe9b75cb") }}</WlButton
      ><WlButton
        variant="primary"
        @click="
          run(() => workspace.commands.saveApp(form), $t('ui.1a8d2ae073')).then(
            (ok) => {
              if (ok) emit('close');
            },
          )
        "
        >{{ $t("ui.b4d30cae52") }}</WlButton
      ></template
    ></WlDialog
  >
</template>
