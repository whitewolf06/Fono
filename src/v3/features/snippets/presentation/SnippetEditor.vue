<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

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
    :header="index === -1 ? $t('ui.d8818ff5b4') : $t('ui.2055179e06')"
    width="560px"
  >
    <div class="form-stack">
      <WlField
        v-slot="field"
        :label="$t('ui.0dd77e98b8')"
        :hint="$t('ui.a5a13d0e43')"
        ><WlInput
          v-bind="field"
          v-model="trigger"
          :maxlength="snippetLimits.triggerChars"
          :placeholder="$t('ui.69396d3e6a')"
      /></WlField>
      <WlField v-slot="field" :label="$t('ui.2ab2e29c5b')"
        ><WlTextarea
          v-bind="field"
          v-model="text"
          :rows="6"
          :maxlength="snippetLimits.textChars"
          :placeholder="$t('ui.a4693c2ef1')"
      /></WlField>
      <p class="muted">{{ $t("ui.95b95d42bc") }}</p>
      <p v-if="text.includes('{clipboard}')" class="notice">
        {{ $t("ui.e5e4b4aa63") }}
      </p>
      <div class="toggle-row">
        <span>{{ $t("ui.cea2dcb8cf") }}</span
        ><WlSwitch v-model="enabled" :aria-label="$t('ui.cea2dcb8cf')" />
      </div>
      <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    </div>
    <template #footer
      ><WlButton @click="close">{{ $t("ui.8fbe9b75cb") }}</WlButton
      ><WlButton variant="primary" @click="submit">{{
        $t("ui.ef82bff588")
      }}</WlButton></template
    >
  </WlDialog>
</template>
