<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

import { computed, ref } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import SelectField from "../../../shared/presentation/SelectField.vue";
import PreferenceToggle from "./PreferenceToggle.vue";
import ProfileManager from "./ProfileManager.vue";
import LlmModelField from "./LlmModelField.vue";
import ProcessingPrompts from "./ProcessingPrompts.vue";
import { translationOptions } from "../../../shared/domain/processing";
defineProps<{ advanced?: boolean }>();
const draft = defineModel<Preferences>({ required: true });
const workspace = useWorkspace();
const { busy, run, error } = useFeedback();
const result = ref("");
const profiles = computed(() =>
  workspace.state.profiles.map((p) => ({ value: p.id, label: p.name })),
);
const selectedProfile = computed(() =>
  workspace.state.profiles.find((p) => p.id === draft.value.profile),
);
const canProcess = computed(() => draft.value.processingEnabled);
const trainerProfile = computed(() =>
  workspace.state.profiles.find((p) => p.id === draft.value.trainerProfile),
);
</script>
<template>
  <div class="form-stack">
    <p v-if="draft.dictationMode === 'live'" class="notice">
      {{ $t("ui.30dd91c84c") }}
    </p>
    <PreferenceToggle
      name="processingEnabled"
      :label="$t('ui.d4a26d3502')"
      :description="$t('ui.a2326dc13c')"
      :disabled="draft.dictationMode === 'live'"
      :effective-value="draft.dictationMode === 'live' ? false : undefined"
    />
    <SelectField
      id="processingTrigger"
      v-model="draft.processingTrigger"
      :label="$t('ui.87486a57b5')"
      :disabled="!canProcess"
      :options="[
        {
          value: 'automatic',
          get label() {
            return $t('ui.a8f986bd2e');
          },
        },
        {
          value: 'manual',
          get label() {
            return $t('ui.edb4ce3079');
          },
        },
      ]"
      :hint="
        draft.processingTrigger === 'manual'
          ? $t('ui.5105a0b7da')
          : $t('ui.73c9e67474')
      "
    />
    <SelectField
      id="processingMode"
      v-model="draft.processingMode"
      :label="$t('ui.c7e41eac12')"
      :disabled="!canProcess"
      :options="[
        {
          value: 'raw',
          get label() {
            return $t('ui.5fee11c377');
          },
        },
        {
          value: 'clean',
          get label() {
            return $t('ui.5a84be0516');
          },
        },
        {
          value: 'format',
          get label() {
            return $t('ui.d8ca68ea68');
          },
        },
        {
          value: 'task',
          get label() {
            return $t('ui.0c77355772');
          },
        },
        {
          value: 'formal',
          get label() {
            return $t('ui.d699682690');
          },
        },
      ]"
      :hint="$t('ui.994580879c')"
    />
    <SelectField
      id="processingTranslation"
      v-model="draft.processingTranslation"
      :label="$t('ui.16b43a4ab6')"
      :disabled="!canProcess"
      :options="[...translationOptions]"
      :hint="
        draft.processingMode === 'raw'
          ? $t('ui.ebf59954bd')
          : $t('ui.0e6bc43604')
      "
    />
    <PreferenceToggle
      name="processingTranslationEnabled"
      :label="$t('ui.73c017a534')"
      :description="$t('ui.2eefdc5961')"
      :disabled="!canProcess || draft.processingTranslation === 'none'"
      :effective-value="
        !canProcess || draft.processingTranslation === 'none'
          ? false
          : undefined
      "
    />
    <p class="muted">{{ $t("ui.2a63b65ce9") }}</p>
    <SelectField
      id="profile"
      v-model="draft.profile"
      :label="$t('ui.c5dc7ac1f2')"
      :options="profiles"
    />
    <div class="connection-address">
      <span>{{ selectedProfile?.url || $t("ui.2dd561616c") }}</span
      ><small>{{
        !selectedProfile
          ? $t("ui.ef1dc7d602")
          : selectedProfile.location === "local"
            ? $t("ui.44e0d61fe1")
            : workspace.native
              ? $t("ui.38c49eea14")
              : $t("ui.1bec7ef688")
      }}</small>
    </div>
    <LlmModelField
      v-model="draft.processingModel"
      :label="$t('ui.c967a64fc4')"
      :profile="draft.profile"
    />
    <WlButton
      size="sm"
      :loading="busy"
      :disabled="!selectedProfile"
      @click="
        run(async () => {
          result = await workspace.settings.testConnection(draft.profile);
        })
      "
      >{{ $t("ui.93275ac8c5") }}</WlButton
    >
    <p v-if="!canProcess" class="muted">
      {{ !draft.processingEnabled ? $t("ui.42b8797f82") : $t("ui.6390e6fbb7") }}
    </p>
    <p v-if="result && !error" class="notice" role="status">{{ result }}</p>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    <template v-if="advanced">
      <div class="section-divider" />
      <ProcessingPrompts v-model="draft" />
      <div class="section-divider" />
      <ProfileManager />
      <div class="section-divider" />
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
    </template>
  </div>
</template>
