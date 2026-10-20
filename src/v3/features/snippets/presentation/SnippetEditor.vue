<script setup lang="ts">
import { computed, ref } from "vue";
import {
  WlDialog,
  WlButton,
  WlField,
  WlInput,
  WlTextarea,
  WlSwitch,
} from "@whitelife-core/ui-kit";
import {
  snippetLimits,
  validateSnippets,
  type Snippet,
} from "../domain/snippets";
import { useFormGuard } from "../../../shared/application/formGuard";
const props = defineProps<{
  entry: Snippet;
  entries: Snippet[];
  index: number;
}>();
const emit = defineEmits<{ close: []; save: [entry: Snippet] }>();
const trigger = ref(props.entry.trigger),
  text = ref(props.entry.text),
  enabled = ref(props.entry.enabled),
  error = ref("");
const discard = useFormGuard(
  () =>
    trigger.value !== props.entry.trigger ||
    text.value !== props.entry.text ||
    enabled.value !== props.entry.enabled,
  () => emit("close"),
);
async function close() {
  if (await discard()) emit("close");
}
const visible = computed({
  get: () => true,
  set: () => {
    void close();
  },
});
function submit() {
  const entry = {
    trigger: trigger.value.trim(),
    text: text.value,
    enabled: enabled.value,
  };
  const entries = props.entries.map((entry) => ({ ...entry }));
  if (props.index === -1) entries.push(entry);
  else entries[props.index] = entry;
  error.value = validateSnippets(entries) || "";
  if (!error.value) emit("save", entry);
}
</script>
<template>
  <WlDialog
    v-model:visible="visible"
    :header="index === -1 ? 'Новый сниппет' : 'Изменить сниппет'"
    width="560px"
  >
    <div class="form-stack">
      <WlField
        v-slot="field"
        label="Фраза-триггер"
        hint="Например, «моя подпись». Должна совпасть вся диктовка."
        ><WlInput
          v-bind="field"
          v-model="trigger"
          :maxlength="snippetLimits.triggerChars"
          placeholder="моя подпись"
      /></WlField>
      <WlField v-slot="field" label="Текст сниппета"
        ><WlTextarea
          v-bind="field"
          v-model="text"
          :rows="6"
          :maxlength="snippetLimits.textChars"
          placeholder="С уважением, Дмитрий"
      /></WlField>
      <p class="muted">
        {date} — дата ДД.ММ.ГГГГ, {time} — время ЧЧ:ММ, {clipboard} — текст из
        буфера. Подстановка выполняется один раз, остальные фигурные скобки
        остаются текстом.
      </p>
      <p v-if="text.includes('{clipboard}')" class="notice">
        При срабатывании сниппета Fono прочитает текстовый буфер. Он станет
        частью результата и локальной истории, если она включена. При
        подстановке ИИ не вызывается.
      </p>
      <div class="toggle-row">
        <span>Сниппет включён</span
        ><WlSwitch v-model="enabled" aria-label="Сниппет включён" />
      </div>
      <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    </div>
    <template #footer
      ><WlButton @click="close">Отмена</WlButton
      ><WlButton variant="primary" @click="submit"
        >Добавить в форму</WlButton
      ></template
    >
  </WlDialog>
</template>
