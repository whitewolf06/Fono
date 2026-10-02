<script setup lang="ts">
import { computed, ref } from "vue";
import { WlButton, WlField, WlTextarea } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import SelectField from "../../../shared/presentation/SelectField.vue";
import PreferenceToggle from "./PreferenceToggle.vue";
import ProfileManager from "./ProfileManager.vue";
import LlmModelField from "./LlmModelField.vue";
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
const trainerProfile = computed(() =>
  workspace.state.profiles.find((p) => p.id === draft.value.trainerProfile),
);
</script>
<template>
  <div class="form-stack">
    <PreferenceToggle
      name="processingEnabled"
      label="Обработка текста"
      description="Убирать речевой мусор или оформлять результат."
    />
    <SelectField
      id="processingMode"
      v-model="draft.processingMode"
      label="Режим"
      :options="[
        { value: 'clean', label: 'Очистка · сохранить ваш стиль' },
        { value: 'format', label: 'Форматирование · структурировать мысли' },
      ]"
    />
    <SelectField
      id="profile"
      v-model="draft.profile"
      label="Профиль подключения"
      :options="profiles"
    />
    <div class="connection-address">
      <span>{{ selectedProfile?.url }}</span
      ><small>{{
        selectedProfile?.location === "local"
          ? "На вашем компьютере"
          : workspace.native
            ? "Облачный профиль"
            : "Облачный профиль · ключ заменён демозначением"
      }}</small>
    </div>
    <LlmModelField
      v-model="draft.processingModel"
      label="Модель для обработки"
      :profile="draft.profile"
    />
    <WlButton
      size="sm"
      :loading="busy"
      @click="
        run(async () => {
          result = await workspace.settings.testConnection(draft.profile);
        })
      "
      >Проверить соединение</WlButton
    >
    <p v-if="result && !error" class="notice" role="status">{{ result }}</p>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    <template v-if="advanced">
      <div class="section-divider" />
      <ProfileManager />
      <div class="section-divider" />
      <h3>Рекомендации речевого тренера</h3>
      <PreferenceToggle
        name="trainerAiEnabled"
        label="Дополнительный разбор через ИИ"
        description="Работает поверх локального анализа привычек. Использует сохранённое подключение."
      />
      <SelectField
        v-model="draft.trainerProfile"
        label="Подключение тренера"
        :options="profiles"
      />
      <LlmModelField
        id="trainerModel"
        v-model="draft.trainerModel"
        label="Модель для рекомендаций"
        :profile="draft.trainerProfile"
      />
      <SelectField
        v-model="draft.trainerScope"
        label="Какие данные передавать"
        :options="[
          { value: 'metrics_only', label: 'Только числовые метрики' },
          { value: 'findings', label: 'Метрики и найденные привычки' },
          { value: 'original_text', label: 'Полная исходная расшифровка' },
        ]"
      />
      <PreferenceToggle
        v-if="trainerProfile?.location === 'cloud'"
        name="cloudConsent"
        label="Разрешить отправку выбранных данных в облако"
        description="Без этого разрешения облачные рекомендации не работают."
      />
      <details id="advanced" class="advanced">
        <summary>Дополнительно · пользовательская инструкция</summary>
        <WlField
          v-slot="field"
          label="Инструкция модели"
          :hint="
            workspace.native
              ? 'Сохраняется локально и отправляется выбранной модели вместе с текстом.'
              : 'Не сохраняется в браузере. Не добавляйте личные данные.'
          "
          ><WlTextarea
            v-bind="field"
            v-model="draft.instruction"
            :rows="4"
            placeholder="Сохраняй смысл и мой стиль. Не добавляй новых фактов."
        /></WlField>
      </details>
    </template>
  </div>
</template>
