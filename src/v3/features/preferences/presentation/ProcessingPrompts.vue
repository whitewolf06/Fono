<script setup lang="ts">
import { uiIntlLocale } from "../../../shared/domain/locale";

import { message as $t } from "../../../shared/domain/messages";

import { computed, onMounted, ref } from "vue";
import { WlButton, WlField, WlTextarea } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import {
  presetOptions,
  type ProcessingPreset,
  type ProcessingPromptCatalog,
} from "../../../shared/domain/processing";
import { useWorkspace } from "../../../shared/application/workspace";
import SelectField from "../../../shared/presentation/SelectField.vue";
import ProcessingPreview from "./ProcessingPreview.vue";
const draft = defineModel<Preferences>({ required: true });
const workspace = useWorkspace();
const selected = ref<ProcessingPreset>(draft.value.processingMode);
const catalog = ref<ProcessingPromptCatalog | null>(null);
const error = ref("");
const loading = ref(false);
const choice = computed(() =>
  selected.value === "raw"
    ? null
    : draft.value.processingPrompts[selected.value],
);
const item = computed(() =>
  catalog.value?.presets.find((p) => p.preset === selected.value),
);
const mode = computed({
  get: () => (choice.value?.useCustom ? "custom" : "default"),
  set: (value) => {
    if (!choice.value) return;
    if (value === "custom" && !choice.value.customPrompt.trim() && item.value)
      choice.value.customPrompt = item.value.defaultPrompt;
    choice.value.useCustom = value === "custom";
  },
});
const testDraft = computed<Preferences>(() => ({
  ...draft.value,
  processingMode: selected.value,
}));
async function load() {
  loading.value = true;
  error.value = "";
  try {
    catalog.value = await workspace.settings.processingPromptCatalog();
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    loading.value = false;
  }
}
onMounted(load);
</script>
<template>
  <section
    id="processingPrompts"
    class="processing-prompts form-stack"
    aria-labelledby="processing-prompts-heading"
  >
    <header>
      <h3 id="processing-prompts-heading">{{ $t("ui.dd215cf090") }}</h3>
      <p class="muted">{{ $t("ui.6445cf62c6") }}</p>
    </header>
    <SelectField
      v-model="selected"
      :label="$t('ui.4ac039c192')"
      :options="[...presetOptions]"
    />
    <p v-if="selected === 'raw'" class="notice">{{ $t("ui.39267cf4bb") }}</p>
    <template v-else-if="choice">
      <SelectField
        v-model="mode"
        :label="$t('ui.77bb330ce4')"
        :disabled="loading || !catalog"
        :options="[
          {
            value: 'default',
            get label() {
              return $t('ui.702ef35636');
            },
          },
          {
            value: 'custom',
            get label() {
              return $t('ui.8392302dd7');
            },
          },
        ]"
      />
      <p class="muted">{{ $t("ui.1efce96332") }}</p>
      <WlField
        v-if="mode === 'custom'"
        v-slot="field"
        :label="$t('ui.cb0b54f594')"
        :hint="workspace.native ? $t('ui.04f522e485') : $t('ui.f9fa714ccf')"
        ><WlTextarea
          v-bind="field"
          v-model="choice.customPrompt"
          :rows="7"
          :placeholder="$t('ui.d834406b81')"
      /></WlField>
      <div v-if="mode === 'custom'" class="processing-prompt-footer">
        <small
          :class="{
            'error-text':
              [...choice.customPrompt].length >
              (catalog?.maxPromptChars || 12000),
          }"
          >{{ [...choice.customPrompt].length.toLocaleString(uiIntlLocale()) }}
          /
          {{
            (catalog?.maxPromptChars || 12000).toLocaleString(uiIntlLocale())
          }}
          {{ $t("ui.67d8e8bef3") }}</small
        ><WlButton size="sm" @click="mode = 'default'">{{
          $t("ui.fd197d0c9b")
        }}</WlButton>
      </div>
      <details v-if="item" class="advanced" :open="mode === 'default'">
        <summary>{{ $t("ui.9fe9e1a395") }} {{ item.label }}</summary>
        <pre class="processing-default-prompt">{{ item.defaultPrompt }}</pre>
      </details>
      <p v-if="mode === 'default' && choice.customPrompt" class="muted">
        {{ $t("ui.2a56960d8f") }}
      </p>
    </template>
    <p v-if="loading" class="muted" role="status">{{ $t("ui.2e266219d7") }}</p>
    <div v-if="error" class="error-text" role="alert">
      {{ error }}
      <WlButton size="sm" @click="load">{{ $t("ui.5189135a61") }}</WlButton>
    </div>
    <details class="advanced">
      <summary>{{ $t("ui.669cca50f2") }}</summary>
      <ul class="processing-prompt-advice">
        <li>{{ $t("ui.70547e6fac") }}</li>
        <li>{{ $t("ui.6eb58a6db9") }}</li>
        <li>{{ $t("ui.6242a9c4b9") }}</li>
        <li>{{ $t("ui.4c552f3b86") }}</li>
        <li>{{ $t("ui.03121a0293") }}</li>
      </ul>
    </details>
    <ProcessingPreview :model-value="testDraft" />
  </section>
</template>
