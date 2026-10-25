<script setup lang="ts">
import { computed } from "vue";
import { message as $t } from "../../../shared/domain/messages";
import type { Preferences } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import SelectField from "../../../shared/presentation/SelectField.vue";
import PreferenceToggle from "./PreferenceToggle.vue";
import LlmModelField from "./LlmModelField.vue";
const draft = defineModel<Preferences>({ required: true });
const workspace = useWorkspace();
const profiles = computed(() =>
  workspace.state.profiles.map((p) => ({ value: p.id, label: p.name })),
);
const trainerProfile = computed(() =>
  workspace.state.profiles.find((p) => p.id === draft.value.trainerProfile),
);
</script>
<template>
  <div class="form-stack">
    <h3>{{ $t("ui.e8e4705211") }}</h3>
    <PreferenceToggle
      name="trainerAiEnabled"
      :label="$t('ui.bff298841f')"
      :description="$t('ui.87950292a3')"
    />
    <SelectField
      v-model="draft.trainerProfile"
      :label="$t('ui.e4a4dc57f1')"
      :options="profiles"
    />
    <LlmModelField
      id="trainerModel"
      v-model="draft.trainerModel"
      :label="$t('ui.206d771bc2')"
      :profile="draft.trainerProfile"
    />
    <SelectField
      v-model="draft.trainerScope"
      :label="$t('ui.9350ccca5d')"
      :options="[
        {
          value: 'metrics_only',
          get label() {
            return $t('ui.9d43c85551');
          },
        },
        {
          value: 'findings',
          get label() {
            return $t('ui.25b482398e');
          },
        },
        {
          value: 'original_text',
          get label() {
            return $t('ui.42595ae665');
          },
        },
      ]"
    />
    <PreferenceToggle
      v-if="trainerProfile?.location === 'cloud'"
      name="cloudConsent"
      :label="$t('ui.cdcc8b7c48')"
      :description="$t('ui.5544981ee8')"
    />
  </div>
</template>
