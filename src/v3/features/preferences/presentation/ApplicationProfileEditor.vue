<script setup lang="ts">
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
  { value: "inherit", label: "Как в общих настройках" },
  { value: "on", label: "Включено" },
  { value: "off", label: "Выключено" },
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
const inherit = { value: "inherit", label: "Как в общих настройках" };
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
  <WlDialog v-model:visible="visible" header="Профиль приложения" width="560px">
    <div class="form-stack">
      <WlField v-slot="field" label="Название профиля"
        ><WlInput
          v-bind="field"
          v-model="draft.name"
          :maxlength="64"
          placeholder="Например, Работа в IDE"
      /></WlField>
      <WlField v-slot="field" label="Приложения профиля"
        ><WlTextarea
          v-bind="field"
          v-model="exeText"
          :maxlength="2064"
          placeholder="code.exe&#10;WindowsTerminal.exe"
          :rows="3"
      /></WlField>
      <p class="muted">
        Только имена exe, по одному в строке. Путь к файлу не нужен. Одно
        приложение может быть в одном профиле.
      </p>
      <SelectField
        v-model="processing"
        label="Обработка текста в профиле"
        :options="booleans"
      />
      <SelectField
        v-model="style"
        label="Стиль в профиле"
        :options="[inherit, ...presetOptions]"
      />
      <SelectField
        v-model="translation"
        label="Перевод в профиле"
        :options="booleans"
      />
      <SelectField
        v-model="targetLanguage"
        label="Переводить на"
        :options="[
          inherit,
          ...translationOptions.filter((p) => p.value !== 'none'),
        ]"
      />
      <SelectField
        v-model="language"
        label="Распознавать на"
        :options="[inherit, ...languages]"
      />
      <SelectField
        v-model="insertion"
        label="Вставка в профиле"
        :options="[
          inherit,
          { value: 'clipboard', label: 'Через буфер обмена' },
          { value: 'sendinput', label: 'Эмуляция нажатий клавиш' },
        ]"
      />
      <p class="muted">
        Ручной выбор в индикаторе важнее профиля. Обработка использует ваше
        сохранённое подключение; облачный провайдер получает текст, если она
        включена.
      </p>
      <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    </div>
    <template #footer
      ><WlButton variant="ghost" @click="close">Отмена</WlButton
      ><WlButton variant="primary" @click="save"
        >Добавить в форму</WlButton
      ></template
    >
  </WlDialog>
</template>
