<script setup lang="ts">
import { computed, ref } from "vue";
import {
  WlDialog,
  WlButton,
  WlField,
  WlInput,
  WlTextarea,
} from "@whitelife-core/ui-kit";
import type { PersonalDictionaryEntry } from "../../../shared/domain/contracts";
import {
  dictionaryLimits,
  validateDictionary,
} from "../../../shared/domain/personalDictionary";
import { useFormGuard } from "../../../shared/application/formGuard";
const props = defineProps<{
  entry: PersonalDictionaryEntry;
  entries: PersonalDictionaryEntry[];
  index: number;
}>();
const emit = defineEmits<{
  close: [];
  save: [entry: PersonalDictionaryEntry];
}>();
const written = ref(props.entry.written);
const spoken = ref(props.entry.spoken.join("\n"));
const error = ref("");
const canDiscard = useFormGuard(
  () =>
    written.value !== props.entry.written ||
    spoken.value !== props.entry.spoken.join("\n"),
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
function submit() {
  const entry = {
    written: written.value.trim(),
    spoken: spoken.value
      .split("\n")
      .map((phrase) => phrase.trim())
      .filter(Boolean),
  };
  const entries = [...props.entries];
  if (props.index === -1) entries.push(entry);
  else entries[props.index] = entry;
  error.value = validateDictionary(entries) || "";
  if (!error.value) emit("save", entry);
}
</script>
<template>
  <WlDialog
    v-model:visible="visible"
    :header="index === -1 ? 'Новое написание' : 'Изменить написание'"
    width="500px"
  >
    <div class="form-stack">
      <WlField
        v-slot="field"
        label="Как писать"
        hint="Точное написание, которое появится в результате."
      >
        <WlInput
          v-bind="field"
          v-model="written"
          :maxlength="dictionaryLimits.phraseChars"
          placeholder="WhiteLife"
        />
      </WlField>
      <WlField
        v-slot="field"
        label="Что распознаётся"
        :hint="`По одному варианту в строке, до ${dictionaryLimits.variants} вариантов. Например, «вайт лайф».`"
      >
        <WlTextarea
          v-bind="field"
          v-model="spoken"
          :rows="4"
          :maxlength="
            dictionaryLimits.variants * (dictionaryLimits.phraseChars + 1)
          "
          placeholder="вайт лайф&#10;white life"
        />
      </WlField>
      <p class="muted">
        Это точные замены, а не обучение модели. Они применяются к целым словам
        и фразам; пунктуация между словами должна совпадать.
      </p>
      <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    </div>
    <template #footer>
      <WlButton @click="close">Отмена</WlButton>
      <WlButton variant="primary" @click="submit">Добавить в форму</WlButton>
    </template>
  </WlDialog>
</template>
