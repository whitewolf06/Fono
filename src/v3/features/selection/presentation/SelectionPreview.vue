<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

import { computed } from "vue";
import { WlButton, WlDialog, WlTextarea } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import SelectionEditor from "./SelectionEditor.vue";
const workspace = useWorkspace();
const current = computed(() => workspace.selection.state.current);
const { run, busy, error } = useFeedback();
function close() {
  const id = current.value?.operationId;
  if (id) void run(() => workspace.selection.cancel(id));
}
</script>
<template>
  <WlDialog
    v-if="current"
    :visible="true"
    :header="$t('ui.f2f1562085')"
    width="640px"
    @update:visible="close"
  >
    <p v-if="current.phase === 'reading'" role="status">
      {{ $t("ui.f368e96ec5") }}
    </p>
    <template v-else-if="current.phase === 'ready'">
      <p class="muted">
        {{
          current.source === "demo"
            ? $t("ui.1e13f12f6f")
            : current.source === "clipboard"
              ? $t("ui.b620555092")
              : $t("ui.a8d994f50c")
        }}
      </p>
      <WlTextarea
        :model-value="current.text"
        readonly
        :rows="4"
        :aria-label="$t('ui.9874f93368')"
      />
      <p class="muted">{{ $t("ui.0816c6a54e") }}</p>
      <SelectionEditor />
      <p v-if="current.historyMessage" role="alert" class="error-text">
        {{ current.historyMessage }}
      </p>
    </template>
    <p v-else role="alert" class="error-text">{{ current.message }}</p>
    <p v-if="error" role="alert" class="error-text">{{ error }}</p>
    <template #footer
      ><WlButton :loading="busy" @click="close">{{
        current.phase === "reading" ? $t("ui.8fbe9b75cb") : $t("ui.a7a4033657")
      }}</WlButton></template
    >
  </WlDialog>
</template>
