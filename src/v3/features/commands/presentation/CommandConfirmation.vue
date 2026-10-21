<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

import { computed } from "vue";
import { WlDialog, WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
const workspace = useWorkspace();
const { run, busy, error } = useFeedback();
const visible = computed({
  get: () => !!workspace.state.commandProposal,
  set: () => {
    void run(() => workspace.commands.dismiss?.());
  },
});
</script>
<template>
  <WlDialog
    v-model:visible="visible"
    :header="$t('ui.48d1d95e44')"
    width="480px"
  >
    <p class="reading-text">{{ workspace.state.commandProposal }}</p>
    <p>{{ $t("ui.8f15f5291c") }}</p>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    <template #footer>
      <WlButton
        :disabled="busy"
        @click="run(() => workspace.commands.dismiss?.())"
        >{{ $t("ui.8fbe9b75cb") }}</WlButton
      >
      <WlButton
        variant="primary"
        :loading="busy"
        @click="run(() => workspace.commands.confirm?.(), $t('ui.1adcea9997'))"
        >{{ $t("ui.50799cfd52") }}</WlButton
      >
    </template>
  </WlDialog>
</template>
