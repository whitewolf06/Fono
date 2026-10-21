<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed, ref } from "vue";
import {
  WlDialog,
  WlField,
  WlInput,
  WlTextarea,
  WlButton,
} from "@whitelife-core/ui-kit";
import type { ApplicationProfile } from "../../../shared/domain/applicationProfiles";
import { validateApplicationProfiles } from "../../../shared/domain/applicationProfiles";
import {
  presetOptions,
  translationOptions,
} from "../../../shared/domain/processing";
import { languages } from "../domain/preferences";
import SelectField from "../../../shared/presentation/SelectField.vue";
import { useFormGuard } from "../../../shared/application/formGuard";
const props = defineProps<{
  profile: ApplicationProfile;
  others: ApplicationProfile[];
}>();
const emit = defineEmits<{ close: []; save: [profile: ApplicationProfile] }>();
const draft = ref({
  ...props.profile,
  executables: [...props.profile.executables],
});
const exeText = ref(props.profile.executables.join("\n"));
const error = ref("");
const canDiscard = useFormGuard(
  () =>
    JSON.stringify({
      ...draft.value,
      executables: exeText.value
        .split(/[\r\n,]+/)
        .map((x) => x.trim())
        .filter(Boolean),
    }) !== JSON.stringify(props.profile),
  () => emit("close"),
);
async function close() {
  if (await canDiscard()) emit("close");
}
const visible = computed({
  get: () => true,
  set: (value: boolean) => {
    if (!value) void close();
  },
});
const booleans = [
  {
    value: "inherit",
    get label() {
      return uiMessage("ui.6887707651");
    },
  },
  {
    value: "on",
    get label() {
      return uiMessage("ui.4d1d43042e");
    },
  },
  {
    value: "off",
    get label() {
      return uiMessage("ui.20494f41ab");
    },
  },
];
const boolChoice = (key: "processingEnabled" | "translationEnabled") =>
  computed({
    get: () =>
      draft.value[key] === null ? "inherit" : draft.value[key] ? "on" : "off",
    set: (value: string) => {
      draft.value[key] = value === "inherit" ? null : value === "on";
    },
  });
const processing = boolChoice("processingEnabled"),
  translation = boolChoice("translationEnabled");
const style = computed({
  get: () => draft.value.preset ?? "inherit",
  set: (v: string) => {
    draft.value.preset =
      v === "inherit" ? null : (v as ApplicationProfile["preset"]);
  },
});
const targetLanguage = computed({
  get: () => draft.value.targetLanguage ?? "inherit",
  set: (v: string) => {
    draft.value.targetLanguage =
      v === "inherit" ? null : (v as ApplicationProfile["targetLanguage"]);
  },
});
const language = computed({
  get: () => draft.value.language ?? "inherit",
  set: (v: string) => {
    draft.value.language = v === "inherit" ? null : v;
  },
});
const insertion = computed({
  get: () => draft.value.injectionMode ?? "inherit",
  set: (v: string) => {
    draft.value.injectionMode =
      v === "inherit" ? null : (v as ApplicationProfile["injectionMode"]);
  },
});
const inherit = {
  value: "inherit",
  get label() {
    return uiMessage("ui.6887707651");
  },
};
function save() {
  const candidate = {
    ...draft.value,
    name: draft.value.name.trim(),
    executables: exeText.value
      .split(/[\r\n,]+/)
      .map((x) => x.trim())
      .filter(Boolean),
  };
  error.value = validateApplicationProfiles([...props.others, candidate]) ?? "";
  if (!error.value) emit("save", candidate);
}
</script>
<template>
  <WlDialog
    v-model:visible="visible"
    :header="$t('ui.16a53dac24')"
    width="560px"
  >
    <div class="form-stack">
      <WlField v-slot="field" :label="$t('ui.c69cc8fffa')"
        ><WlInput
          v-bind="field"
          v-model="draft.name"
          :maxlength="64"
          :placeholder="$t('ui.bce627d8e5')"
      /></WlField>
      <WlField v-slot="field" :label="$t('ui.c076b845d2')"
        ><WlTextarea
          v-bind="field"
          v-model="exeText"
          :maxlength="2064"
          placeholder="code.exe&#10;WindowsTerminal.exe"
          :rows="3"
      /></WlField>
      <p class="muted">{{ $t("ui.2fdeb218b9") }}</p>
      <SelectField
        v-model="processing"
        :label="$t('ui.9e8a74a918')"
        :options="booleans"
      />
      <SelectField
        v-model="style"
        :label="$t('ui.3a213de1f9')"
        :options="[inherit, ...presetOptions]"
      />
      <SelectField
        v-model="translation"
        :label="$t('ui.3f9b877898')"
        :options="booleans"
      />
      <SelectField
        v-model="targetLanguage"
        :label="$t('ui.7f3c909a66')"
        :options="[
          inherit,
          ...translationOptions.filter((p) => p.value !== 'none'),
        ]"
      />
      <SelectField
        v-model="language"
        :label="$t('ui.5450f822e8')"
        :options="[inherit, ...languages]"
      />
      <SelectField
        v-model="insertion"
        :label="$t('ui.9231e94899')"
        :options="[
          inherit,
          {
            value: 'clipboard',
            get label() {
              return $t('ui.7c94ff25a0');
            },
          },
          {
            value: 'sendinput',
            get label() {
              return $t('ui.f3cc63fc58');
            },
          },
        ]"
      />
      <p class="muted">{{ $t("ui.9731e423de") }}</p>
      <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    </div>
    <template #footer
      ><WlButton variant="ghost" @click="close">{{
        $t("ui.8fbe9b75cb")
      }}</WlButton
      ><WlButton variant="primary" @click="save">{{
        $t("ui.ef82bff588")
      }}</WlButton></template
    >
  </WlDialog>
</template>
