<script setup lang="ts">
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
    header="Выполнить голосовую команду?"
    width="480px"
  >
    <p class="reading-text">{{ workspace.state.commandProposal }}</p>
    <p>Действие будет выполнено только после подтверждения.</p>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    <template #footer>
      <WlButton
        :disabled="busy"
        @click="run(() => workspace.commands.dismiss?.())"
        >Отмена</WlButton
      >
      <WlButton
        variant="primary"
        :loading="busy"
        @click="run(() => workspace.commands.confirm?.(), 'Команда выполнена')"
        >Выполнить</WlButton
      >
    </template>
  </WlDialog>
</template>
