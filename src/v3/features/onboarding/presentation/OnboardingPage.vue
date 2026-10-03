<script setup lang="ts">
import { computed, ref } from "vue";
import { useRouter, onBeforeRouteLeave } from "vue-router";
import { WlButton } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import {
  useDraft,
  AudioFields,
  ActivationFields,
  ProcessingFields,
  ModelManager,
} from "../../preferences";
import PageHeading from "../../../shared/presentation/PageHeading.vue";

import { RecordControl } from "../../dictation";
const workspace = useWorkspace();
const router = useRouter();
const { run, busy, error } = useFeedback();
const step = computed(() => workspace.state.onboardingStep);
const names = ["Микрофон", "Распознавание", "Активация", "Обработка", "Проба"];
const fields: (keyof Preferences)[][] = [
  ["microphone"],
  ["model", "language", "acceleration"],
  [
    "hotkey",
    "commandHotkey",
    "dictationMode",
    "wakePhrase",
    "wakeLanguage",
    "silenceMs",
  ],
  ["processingMode", "profile", "processingModel"],
  [],
];
const { draft, dirty, save, canLeave, reset } = useDraft(
  () => fields[step.value],
);
const initialSession = ref(workspace.state.last.entry?.id);
const tried = computed(
  () => workspace.state.last.entry?.id !== initialSession.value,
);
const ready = computed(() =>
  step.value === 0
    ? workspace.state.microphoneAvailable
    : step.value === 1
      ? workspace.state.models.some(
          (m) => m.id === draft.model && m.status === "installed",
        )
      : true,
);
onBeforeRouteLeave(canLeave);
async function next() {
  if (
    !(await run(async () => {
      if (dirty.value) await save();
    }))
  )
    return;
  if (step.value < 4) workspace.resumeOnboarding(step.value + 1);
  else {
    workspace.resumeOnboarding(0);
    await router.push("/");
  }
}
async function back() {
  if (!(await canLeave())) return;
  reset();
  workspace.resumeOnboarding(Math.max(0, step.value - 1));
}
async function skip() {
  if (await run(() => workspace.settings.toggle("processingEnabled", false))) {
    reset();
    workspace.resumeOnboarding(4);
  }
}
</script>
<template>
  <div class="page onboarding-page">
    <PageHeading
      title="Знакомство с Fono"
      description="Пять коротких шагов — и можно говорить."
      ><RouterLink class="text-link" to="/"
        >Продолжить позже</RouterLink
      ></PageHeading
    >
    <ol class="wizard-steps">
      <li
        v-for="(name, index) in names"
        :key="name"
        :class="{ current: step === index, complete: step > index }"
        :aria-current="step === index ? 'step' : undefined"
      >
        <span>{{ step > index ? "✓" : index + 1 }}</span
        ><small>{{ name }}</small>
      </li>
    </ol>
    <section class="wizard-content">
      <small class="eyebrow">Шаг {{ step + 1 }} из 5</small>
      <h2>
        {{
          [
            "Давайте вас услышим",
            "Выберите модель речи",
            "Как начнём диктовку?",
            "Чуть чище, чуть понятнее",
            "Попробуйте прямо сейчас",
          ][step]
        }}
      </h2>
      <AudioFields v-if="step === 0" v-model="draft" scope="microphone" />
      <template v-if="step === 1"
        ><AudioFields v-model="draft" scope="recognition" /><ModelManager
      /></template>
      <ActivationFields v-if="step === 2" v-model="draft" />
      <template v-if="step === 3"
        ><p class="muted">
          Необязательный шаг. Fono умеет записывать речь и без обработки через
          ИИ.
        </p>
        <ProcessingFields v-model="draft"
      /></template>
      <template v-if="step === 4"
        ><p>
          Нажмите «Начать запись», произнесите фразу, затем «Завершить».
          {{
            workspace.native
              ? "Ниже появится распознанный текст."
              : "В демонстрации появится подготовленный текст."
          }}
        </p>
        <RecordControl />
        <div v-if="tried" class="trial-result">
          <small class="eyebrow">Получилось</small>
          <p>{{ workspace.state.last.draft }}</p>
        </div></template
      >
      <p v-if="error" role="alert" class="error-text">{{ error }}</p>
      <footer class="wizard-footer">
        <WlButton v-if="step" :disabled="busy" @click="back">Назад</WlButton
        ><WlButton
          v-if="step === 3"
          variant="ghost"
          :disabled="busy"
          @click="skip"
          >Пока без обработки</WlButton
        ><WlButton
          variant="primary"
          class="push-right"
          :loading="busy"
          :disabled="!ready || (step === 4 && !tried)"
          @click="next"
          >{{ step === 4 ? "Готово, на главную" : "Продолжить" }}</WlButton
        >
      </footer>
    </section>
  </div>
</template>
