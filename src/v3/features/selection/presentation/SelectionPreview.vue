<script setup lang="ts">
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
    header="Выделенный текст"
    width="640px"
    @update:visible="close"
  >
    <p v-if="current.phase === 'reading'" role="status">
      Получаю выделенный фрагмент… Повторное сочетание отменяет получение.
    </p>
    <template v-else-if="current.phase === 'ready'">
      <p class="muted">
        {{
          current.source === "demo"
            ? "Демонстрационный фрагмент"
            : current.source === "clipboard"
              ? "Получено через резервный Ctrl+C"
              : "Получено через UI Automation"
        }}
      </p>
      <WlTextarea
        :model-value="current.text"
        readonly
        :rows="4"
        aria-label="Полученный выделенный текст"
      />
      <p class="muted">
        Фрагмент хранится только в памяти Fono до закрытия. В историю он не
        записывается. Отправка в ИИ запускается отдельной кнопкой ниже.
      </p>
      <SelectionEditor />
    </template>
    <p v-else role="alert" class="error-text">{{ current.message }}</p>
    <p v-if="error" role="alert" class="error-text">{{ error }}</p>
    <template #footer
      ><WlButton :loading="busy" @click="close">{{
        current.phase === "reading" ? "Отмена" : "Закрыть"
      }}</WlButton></template
    >
  </WlDialog>
</template>
