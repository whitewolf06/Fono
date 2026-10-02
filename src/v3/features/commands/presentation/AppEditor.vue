<script setup lang="ts">
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
    :header="application.id ? 'Изменить приложение' : 'Добавить приложение'"
    width="500px"
    ><div class="form-stack">
      <WlField v-slot="field" label="Название"
        ><WlInput
          v-bind="field"
          v-model="form.name"
          placeholder="Например, Блокнот" /></WlField
      ><WlField v-slot="field" label="Произносимая фраза"
        ><WlInput
          v-bind="field"
          v-model="form.phrase"
          placeholder="открой блокнот" /></WlField
      ><WlField v-slot="field" label="Путь или команда запуска"
        ><WlInput v-bind="field" v-model="form.path" placeholder="notepad.exe"
      /></WlField>
      <p class="muted">
        Укажите одно или несколько названий через запятую. Команда: «открой» и
        название приложения.
      </p>
      <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    </div>
    <template #footer
      ><WlButton @click="close">Отмена</WlButton
      ><WlButton
        variant="primary"
        @click="
          run(
            () => workspace.commands.saveApp(form),
            'Приложение сохранено',
          ).then((ok) => {
            if (ok) emit('close');
          })
        "
        >Сохранить</WlButton
      ></template
    ></WlDialog
  >
</template>
