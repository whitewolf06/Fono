<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

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
    :header="profile.id ? $t('ui.c5dc7ac1f2') : $t('ui.1770ea4ae6')"
    width="510px"
    ><div class="form-stack">
      <WlField v-slot="field" :label="$t('ui.0918b4ba92')"
        ><WlInput
          v-bind="field"
          v-model="form.name"
          :placeholder="$t('ui.ceb82bc77a')"
      /></WlField>
      <div class="form-grid">
        <SelectField
          v-model="form.provider"
          :label="$t('ui.5592a52ad7')"
          :options="[
            { value: 'lmstudio', label: 'LM Studio' },
            { value: 'openai', label: 'OpenAI' },
            {
              value: 'custom',
              get label() {
                return $t('ui.ecafdf9d1e');
              },
            },
          ]"
        /><SelectField
          v-model="form.location"
          :label="$t('ui.a70a9ff4fc')"
          :options="[
            {
              value: 'local',
              get label() {
                return $t('ui.d5077a2034');
              },
            },
            {
              value: 'cloud',
              get label() {
                return $t('ui.b09d21d043');
              },
            },
          ]"
        />
      </div>
      <WlField v-slot="field" :label="$t('ui.4289b17137')"
        ><WlInput
          v-bind="field"
          v-model="form.url"
          placeholder="http://127.0.0.1:1234/v1"
      /></WlField>
      <WlField v-slot="field" :label="$t('ui.ce91e2c532')"
        ><WlInput
          v-bind="field"
          v-model="form.model"
          :placeholder="$t('ui.2eea50c922')"
      /></WlField>
      <WlField
        v-if="workspace.native"
        v-slot="field"
        :label="$t('ui.5ced5bd906')"
        :hint="form.hasApiKey ? $t('ui.1a6d134b0e') : $t('ui.b3629fb8bd')"
        ><WlInput
          v-bind="field"
          v-model="form.apiKey"
          type="password"
          autocomplete="new-password"
      /></WlField>
      <p v-if="!workspace.native" class="notice">{{ $t("ui.347f51d698") }}</p>
      <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    </div>
    <template #footer
      ><WlButton :disabled="busy" @click="close">{{
        $t("ui.8fbe9b75cb")
      }}</WlButton
      ><WlButton
        variant="primary"
        :loading="busy"
        @click="
          run(
            () => workspace.settings.saveProfile(form),
            $t('ui.d7ae5c2bcd'),
          ).then((ok) => {
            if (ok) emit('close');
          })
        "
        >{{ $t("ui.1e62f28f43") }}</WlButton
      ></template
    ></WlDialog
  >
</template>
