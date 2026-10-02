<script setup lang="ts">
import { computed, reactive } from "vue";
import { WlDialog, WlField, WlInput, WlButton } from "@whitelife-core/ui-kit";
import type { ConnectionProfile } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFormGuard } from "../../../shared/application/formGuard";
import { useFeedback } from "../../../shared/application/feedback";
import SelectField from "../../../shared/presentation/SelectField.vue";
const props = defineProps<{ profile: ConnectionProfile }>();
const emit = defineEmits<{ close: [] }>();
const form = reactive({ ...props.profile });
const workspace = useWorkspace();

const { busy, error, run } = useFeedback();
const canDiscard = useFormGuard(
  () => JSON.stringify(form) !== JSON.stringify(props.profile),
  () => emit("close"),
  () => busy.value,
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
    :header="profile.id ? 'Профиль подключения' : 'Новое подключение'"
    width="510px"
    ><div class="form-stack">
      <WlField v-slot="field" label="Название"
        ><WlInput
          v-bind="field"
          v-model="form.name"
          placeholder="Мой локальный сервер"
      /></WlField>
      <div class="form-grid">
        <SelectField
          v-model="form.provider"
          label="Провайдер"
          :options="[
            { value: 'lmstudio', label: 'LM Studio' },
            { value: 'openai', label: 'OpenAI' },
            { value: 'custom', label: 'Совместимый API' },
          ]"
        /><SelectField
          v-model="form.location"
          label="Расположение"
          :options="[
            { value: 'local', label: 'На компьютере' },
            { value: 'cloud', label: 'В облаке' },
          ]"
        />
      </div>
      <WlField v-slot="field" label="Адрес сервера"
        ><WlInput
          v-bind="field"
          v-model="form.url"
          placeholder="http://127.0.0.1:1234/v1"
      /></WlField>
      <WlField v-slot="field" label="Модель по умолчанию"
        ><WlInput
          v-bind="field"
          v-model="form.model"
          placeholder="Название модели"
      /></WlField>
      <WlField
        v-if="workspace.native"
        v-slot="field"
        label="API-ключ"
        :hint="
          form.hasApiKey
            ? 'Ключ сохранён. Оставьте пустым, чтобы сохранить его.'
            : 'Для локального сервера обычно не нужен.'
        "
        ><WlInput
          v-bind="field"
          v-model="form.apiKey"
          type="password"
          autocomplete="new-password"
      /></WlField>
      <p v-if="!workspace.native" class="notice">
        Это настройка демонстрационного подключения. Запросы не отправляются,
        ключ заменён заглушкой. Изменённые профили действуют до перезагрузки
        страницы.
      </p>
      <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    </div>
    <template #footer
      ><WlButton :disabled="busy" @click="close">Отмена</WlButton
      ><WlButton
        variant="primary"
        :loading="busy"
        @click="
          run(
            () => workspace.settings.saveProfile(form),
            'Профиль сохранён',
          ).then((ok) => {
            if (ok) emit('close');
          })
        "
        >Сохранить профиль</WlButton
      ></template
    ></WlDialog
  >
</template>
