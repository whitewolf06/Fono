<script setup lang="ts">
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
      В живой диктовке обработка через ИИ выключена. Эти параметры применяются к
      обычной диктовке.
    </p>
    <PreferenceToggle
      name="processingEnabled"
      label="Обработка текста"
      description="Убирать речевой мусор или оформлять результат."
      :disabled="draft.dictationMode === 'live'"
      :effective-value="draft.dictationMode === 'live' ? false : undefined"
    />
    <SelectField
      id="processingTrigger"
      v-model="draft.processingTrigger"
      label="Когда обрабатывать"
      :disabled="!canProcess"
      :options="[
        { value: 'automatic', label: 'Автоматически после диктовки' },
        { value: 'manual', label: 'По кнопке в индикаторе' },
      ]"
      :hint="
        draft.processingTrigger === 'manual'
          ? 'После распознавания Fono ждёт вашего решения. Галочка в индикаторе оставляет результат для копирования; повторное нажатие сочетания вставляет текст с выбранными настройками.'
          : 'Fono применит текущие настройки. Повторное нажатие сочетания вставляет результат; галочка оставляет его для копирования без автоматической вставки.'
      "
    />
    <SelectField
      id="processingMode"
      v-model="draft.processingMode"
      label="Стиль обработки по умолчанию"
      :disabled="!canProcess"
      :options="[
        {
          value: 'raw',
          label: 'Без изменений · только распознавание или перевод',
        },
        { value: 'clean', label: 'Очистка · сохранить ваш стиль' },
        { value: 'format', label: 'Форматирование · структурировать мысли' },
        { value: 'task', label: 'Постановка задачи · цель и шаги' },
        { value: 'formal', label: 'Деловое письмо · сдержанный тон' },
      ]"
      hint="Стиль можно менять в индикаторе. Выбор запоминается для следующих диктовок."
    />
    <SelectField
      id="processingTranslation"
      v-model="draft.processingTranslation"
      label="Язык перевода"
      :disabled="!canProcess"
      :options="[...translationOptions]"
      :hint="
        draft.processingMode === 'raw'
          ? 'Без изменений: выполняется только перевод. Без перевода запрос к модели не отправляется.'
          : 'ИИ редактирует текст и переводит его с сохранением смысла.'
      "
    />
    <PreferenceToggle
      name="processingTranslationEnabled"
      label="Применять перевод"
      description="Язык запоминается даже при выключенном переводе."
      :disabled="!canProcess || draft.processingTranslation === 'none'"
      :effective-value="
        !canProcess || draft.processingTranslation === 'none'
          ? false
          : undefined
      "
    />
    <p class="muted">
      В режиме повторного нажатия быстрые настройки индикатора позволяют сменить
      стиль и перевод до завершения записи. Они запоминаются и работают как с
      автоматической обработкой, так и с обработкой по кнопке.
    </p>
    <SelectField
      id="profile"
      v-model="draft.profile"
      label="Профиль подключения"
      :options="profiles"
    />
    <div class="connection-address">
      <span>{{ selectedProfile?.url || "Подключение ещё не настроено" }}</span
      ><small>{{
        !selectedProfile
          ? "Создайте подключение в общих настройках"
          : selectedProfile.location === "local"
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
      :disabled="!selectedProfile"
      @click="
        run(async () => {
          result = await workspace.settings.testConnection(draft.profile);
        })
      "
      >Проверить соединение</WlButton
    >
    <p v-if="!canProcess" class="muted">
      {{
        !draft.processingEnabled
          ? "Включите обработку, чтобы выбирать стиль и перевод."
          : "Выберите подключение и модель. Управление подключениями — в разделе «Все настройки»."
      }}
    </p>
    <p v-if="result && !error" class="notice" role="status">{{ result }}</p>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    <template v-if="advanced">
      <div class="section-divider" />
      <ProcessingPrompts v-model="draft" />
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
    </template>
  </div>
</template>
