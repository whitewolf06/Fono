<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed, onMounted, ref } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import type { WakeValidationKind } from "../../../shared/domain/wake";
import { WAKE_WORD_AVAILABLE } from "../../../shared/domain/wakeAvailability";
const props = defineProps<{ phrase: string; language: "ru" | "en" }>();
const workspace = useWorkspace();
const { run, busy, error } = useFeedback();
const result = ref("");
const setup = computed(() => workspace.state.wakeSetup);
const changed = computed(
  () =>
    props.phrase !== workspace.state.preferences.wakePhrase ||
    props.language !== workspace.state.preferences.wakeLanguage,
);
const reasons: Record<string, string> = {
  get silence() {
    return uiMessage("ui.e7a9c7edf2");
  },
  get clipping() {
    return uiMessage("ui.bd6135b946");
  },
  get too_short() {
    return uiMessage("ui.ab6846df6a");
  },
  get phrase_not_detected() {
    return uiMessage("ui.2d4d0bc35b");
  },
};
const reason = computed(
  () =>
    reasons[setup.value?.latest?.reason || ""] || uiMessage("ui.e147b82396"),
);
async function savePhrase() {
  await workspace.settings.save({
    wakePhrase: props.phrase.trim(),
    wakeLanguage: props.language,
  });
}
async function begin() {
  await run(async () => {
    if (workspace.state.preferences.wakeEnabled)
      await workspace.settings.toggle("wakeEnabled", false);
    await savePhrase();
    await workspace.wake!.load();
    if (!workspace.state.wakeSetup?.modelReady)
      await workspace.wake!.download();
    await workspace.wake!.begin();
  });
}
async function validate(kind: WakeValidationKind) {
  await run(() => workspace.wake!.validate(kind));
}
onMounted(() => {
  if (WAKE_WORD_AVAILABLE) void run(() => workspace.wake!.load());
});
</script>
<template>
  <section
    v-if="workspace.wake && WAKE_WORD_AVAILABLE"
    class="wake-setup form-stack"
    :aria-label="$t('ui.c82ad8d5a7')"
  >
    <p class="muted">
      {{ workspace.native ? $t("ui.f23f58acba") : $t("ui.da2b32729a") }}
    </p>
    <p v-if="setup?.verified && !changed" class="notice" role="status">
      {{ $t("ui.892840af4b") }}{{ setup.phrase }}{{ $t("ui.345c9eebb3") }}
    </p>
    <WlButton
      v-if="
        setup?.verified && !changed && !workspace.state.preferences.wakeEnabled
      "
      size="sm"
      :disabled="busy"
      @click="run(() => workspace.settings.toggle('wakeEnabled', true))"
      >{{ $t("ui.b956dde287") }}</WlButton
    >
    <div class="actions">
      <WlButton size="sm" :disabled="busy || !phrase.trim()" @click="begin">{{
        setup?.profileReady ? $t("ui.50f247b205") : $t("ui.f154130341")
      }}</WlButton>
      <WlButton
        size="sm"
        variant="ghost"
        :disabled="busy || !setup?.modelReady || changed"
        @click="
          run(async () => {
            result = await workspace.wake!.test();
          })
        "
        >{{ $t("ui.37eafb384a") }}</WlButton
      >
    </div>
    <div v-if="setup?.active" class="surface-panel form-stack">
      <h4>{{ $t("ui.754824019c") }}{{ setup.phrase }}»</h4>
      <progress
        :value="setup.accepted"
        :max="setup.required"
        :aria-label="
          $t('ui.6794f16101') +
          setup.accepted +
          $t('ui.e029cc4b39') +
          setup.required
        "
      />
      <p>
        {{ setup.accepted }} {{ $t("ui.7f4adf318e") }} {{ setup.required }}
        {{ $t("ui.d8d8026f93") }}
      </p>
      <p
        v-if="setup.latest && !setup.latest.accepted"
        class="error-text"
        role="status"
      >
        {{ $t("ui.4a8d270b00") }} {{ reason }}{{ $t("ui.99e3db2a3b") }}
      </p>
      <WlButton
        size="sm"
        :loading="busy"
        :disabled="changed"
        @click="run(() => workspace.wake!.record())"
        >{{ $t("ui.8953ab1d04") }}</WlButton
      >
    </div>
    <div
      v-if="setup?.profileReady && !setup.active && !setup.verified && !changed"
      class="surface-panel form-stack"
    >
      <h4>{{ $t("ui.9e803a74de") }}</h4>
      <p>{{ $t("ui.c0206cba30") }}</p>
      <WlButton
        v-if="!setup.validation.active"
        size="sm"
        :disabled="busy"
        @click="run(() => workspace.wake!.beginValidation())"
        >{{
          setup.validation.failed ? $t("ui.40f9ba8446") : $t("ui.29ed524062")
        }}</WlButton
      >
      <template v-else>
        <p>
          {{ $t("ui.caaaea3c81") }} {{ setup.validation.positivePassed }}/{{
            setup.validation.positiveRequired
          }}
          {{ $t("ui.9bea031b8e") }}
          {{
            setup.validation.silencePassed
              ? $t("ui.f32311d0e2")
              : $t("ui.c677b1bcc1")
          }}
          {{ $t("ui.1fcc8d8839") }}
          {{
            setup.validation.otherPhrasePassed
              ? $t("ui.f32311d0e2")
              : $t("ui.c677b1bcc1")
          }}
        </p>
        <div class="actions">
          <WlButton
            size="sm"
            :disabled="
              busy ||
              setup.validation.positivePassed >=
                setup.validation.positiveRequired
            "
            @click="validate('positive')"
            >{{ $t("ui.d8c480d70c") }}</WlButton
          >
          <WlButton
            size="sm"
            :disabled="busy || setup.validation.silencePassed"
            @click="validate('silence')"
            >{{ $t("ui.1007c68320") }}</WlButton
          >
          <WlButton
            size="sm"
            :disabled="busy || setup.validation.otherPhrasePassed"
            @click="validate('other_phrase')"
            >{{ $t("ui.69086656d7") }}</WlButton
          >
        </div>
        <p class="muted">{{ $t("ui.4461f1419b") }}</p>
      </template>
      <p v-if="setup.validation.failed" class="error-text" role="alert">
        {{ $t("ui.0af0402c04") }}
      </p>
    </div>
    <div v-if="setup?.active || setup?.validation.active" class="actions">
      <WlButton
        size="sm"
        variant="ghost"
        :disabled="busy"
        @click="run(() => workspace.wake!.cancel())"
        >{{ $t("ui.c6085f7f11") }}</WlButton
      >
    </div>
    <p v-if="changed && (setup?.active || setup?.profileReady)" class="notice">
      {{ $t("ui.6cf68491dd") }}
    </p>
    <p v-if="busy" role="status">{{ $t("ui.185e915cca") }}</p>
    <p v-if="result" class="notice" role="status">{{ result }}</p>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
  </section>
  <WlButton v-else size="sm" disabled>{{ $t("ui.275e0db909") }}</WlButton>
</template>
