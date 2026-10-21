<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed, ref, watch } from "vue";
import { WlButton, WlField, WlInput } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import SelectField from "../../../shared/presentation/SelectField.vue";
const props = defineProps<{ profile: string; label: string }>();
const model = defineModel<string>({ required: true });
const workspace = useWorkspace();
const { busy, run, error } = useFeedback();
const available = ref<string[]>([]);
const manual = ref(true);
const options = computed(() =>
  [
    ...new Set([
      model.value,
      ...available.value,
      ...(!workspace.native
        ? ["Qwen 3 · 8B", "GPT-4o Mini", "Llama 3.1 · 8B"]
        : []),
    ]),
  ]
    .filter(Boolean)
    .map((value) => ({ value, label: value })),
);
watch(
  () => props.profile,
  () => {
    available.value = [];
    manual.value = true;
    model.value =
      workspace.state.profiles.find((p) => p.id === props.profile)?.model || "";
  },
);
async function load() {
  const profile = props.profile;
  await run(async () => {
    const result = await workspace.settings.listModels!(profile);
    if (props.profile !== profile) return;
    if (!result.length) throw new Error(uiMessage("ui.8c432ecee1"));
    available.value = result;
    manual.value = false;
  });
}
</script>
<template>
  <div class="form-stack">
    <WlField
      v-if="workspace.native && manual"
      v-slot="field"
      :label="label"
      :hint="$t('ui.a227eee4d6')"
    >
      <WlInput
        v-bind="field"
        v-model="model"
        :placeholder="$t('ui.6e055a0bc4')"
      />
    </WlField>
    <SelectField v-else v-model="model" :label="label" :options="options" />
    <div v-if="workspace.native" class="actions">
      <WlButton
        size="sm"
        variant="ghost"
        :disabled="!profile"
        :loading="busy"
        @click="load"
        >{{ $t("ui.16ecaaa940") }}</WlButton
      >
      <WlButton
        v-if="!manual"
        size="sm"
        variant="ghost"
        @click="manual = true"
        >{{ $t("ui.7b9139695c") }}</WlButton
      >
    </div>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
  </div>
</template>
