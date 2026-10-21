<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

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
const names = [
  uiMessage("ui.f038415295"),
  uiMessage("ui.7894eb1ac6"),
  uiMessage("ui.c7f4e5b5d8"),
  uiMessage("ui.fd0ecd47a7"),
  uiMessage("ui.6728b964ef"),
];
const fields: (keyof Preferences)[][] = [
  ["microphone"],
  ["model", "language", "acceleration"],
  [
    "hotkey",
    "dictationTrigger",
    "hotkeyMode",
    "commandHotkey",
    "dictationMode",
    "wakePhrase",
    "wakeLanguage",
    "silenceMs",
  ],
  [
    "processingTrigger",
    "processingMode",
    "processingTranslation",
    "profile",
    "processingModel",
  ],
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
    <PageHeading :title="$t('ui.3916e395f3')" :description="$t('ui.b38752c852')"
      ><RouterLink class="text-link" to="/">{{
        $t("ui.25cf8f2e2b")
      }}</RouterLink></PageHeading
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
      <small class="eyebrow"
        >{{ $t("ui.1964891c92") }} {{ step + 1 }}
        {{ $t("ui.0fcfbc0cbd") }}</small
      >
      <h2>
        {{
          [
            $t("ui.8d6018a95e"),
            $t("ui.3a4cd32e26"),
            $t("ui.36029a2eb1"),
            $t("ui.6b4c634eae"),
            $t("ui.378499abbb"),
          ][step]
        }}
      </h2>
      <AudioFields v-if="step === 0" v-model="draft" scope="microphone" />
      <template v-if="step === 1"
        ><AudioFields v-model="draft" scope="recognition" /><ModelManager
      /></template>
      <ActivationFields v-if="step === 2" v-model="draft" />
      <template v-if="step === 3"
        ><p class="muted">{{ $t("ui.60312f8a50") }}</p>
        <ProcessingFields v-model="draft"
      /></template>
      <template v-if="step === 4"
        ><p>
          {{ $t("ui.99b2836ff3") }}
          {{
            workspace.state.preferences.processingEnabled &&
            workspace.state.preferences.processingTrigger === "manual"
              ? $t("ui.66a00e614e")
              : ""
          }}
          {{ workspace.native ? $t("ui.5e9067f3da") : $t("ui.190c8ec1e5") }}
        </p>
        <RecordControl />
        <div v-if="tried" class="trial-result">
          <small class="eyebrow">{{ $t("ui.f7d7fb0081") }}</small>
          <p>{{ workspace.state.last.draft }}</p>
        </div></template
      >
      <p v-if="error" role="alert" class="error-text">{{ error }}</p>
      <footer class="wizard-footer">
        <WlButton v-if="step" :disabled="busy" @click="back">{{
          $t("ui.1a9fb1f3cf")
        }}</WlButton
        ><WlButton
          v-if="step === 3"
          variant="ghost"
          :disabled="busy"
          @click="skip"
          >{{ $t("ui.98772bfe52") }}</WlButton
        ><WlButton
          variant="primary"
          class="push-right"
          :loading="busy"
          :disabled="!ready || (step === 4 && !tried)"
          @click="next"
          >{{
            step === 4 ? $t("ui.6c8460ee51") : $t("ui.a42aee58d6")
          }}</WlButton
        >
      </footer>
    </section>
  </div>
</template>
