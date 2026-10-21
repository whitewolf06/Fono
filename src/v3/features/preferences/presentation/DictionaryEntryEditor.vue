<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

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
    :header="index === -1 ? $t('ui.61b3da3433') : $t('ui.bc0fac0aa9')"
    width="500px"
  >
    <div class="form-stack">
      <WlField
        v-slot="field"
        :label="$t('ui.5e94bcf418')"
        :hint="$t('ui.2f9eb70339')"
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
        :label="$t('ui.e80b4cfacf')"
        :hint="$t('ui.b14f2ccceb', { v0: String(dictionaryLimits.variants) })"
      >
        <WlTextarea
          v-bind="field"
          v-model="spoken"
          :rows="4"
          :maxlength="
            dictionaryLimits.variants * (dictionaryLimits.phraseChars + 1)
          "
          :placeholder="$t('ui.63cdbcf1f5')"
        />
      </WlField>
      <p class="muted">{{ $t("ui.32f67100e6") }}</p>
      <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    </div>
    <template #footer>
      <WlButton @click="close">{{ $t("ui.8fbe9b75cb") }}</WlButton>
      <WlButton variant="primary" @click="submit">{{
        $t("ui.ef82bff588")
      }}</WlButton>
    </template>
  </WlDialog>
</template>
