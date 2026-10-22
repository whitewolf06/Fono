<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive } from "vue";
import { WlButton, WlCheckbox } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { message as $t } from "../../../shared/domain/messages";
import SelectField from "../../../shared/presentation/SelectField.vue";
import type { WakeSampleRequest } from "../domain/samples";

const workspace = useWorkspace();
const port = workspace.wakeSamples;
const state = port.state;
const request = reactive<WakeSampleRequest>({
  language: "ru",
  kind: "phrase",
  condition: "quiet",
  consent: false,
});
const active = computed(() => state.phase !== "idle");
const blocked = computed(
  () =>
    !workspace.native ||
    active.value ||
    !["idle", "error"].includes(workspace.state.phase),
);
const phrase = computed(() =>
  request.language === "ru" ? "Эй, Фоно" : "Hey Fono",
);
const selected = computed(() =>
  state.samples.filter(
    (s) =>
      s.language === request.language &&
      s.kind === request.kind &&
      s.condition === request.condition,
  ),
);
const target = computed(() =>
  request.kind === "phrase" ? 5 : request.kind === "other_speech" ? 3 : 1,
);
const prompt = computed(() =>
  request.kind === "phrase"
    ? $t("wakeSamples.sayPhrase", { phrase: phrase.value })
    : $t(
        request.kind === "other_speech"
          ? "wakeSamples.otherPrompt"
          : "wakeSamples.backgroundPrompt",
      ),
);
const kindLabel = (kind: WakeSampleRequest["kind"]) =>
  $t(
    kind === "phrase"
      ? "wakeSamples.phrase"
      : kind === "other_speech"
        ? "wakeSamples.other"
        : "wakeSamples.background",
  );
onMounted(() => void port.load());
onUnmounted(() => void port.cancel());
</script>
<template>
  <section
    id="wake-samples"
    class="form-stack wake-samples"
    :aria-label="$t('wakeSamples.title')"
  >
    <h3>{{ $t("wakeSamples.title") }}</h3>
    <p class="muted">{{ $t("wakeSamples.explanation") }}</p>
    <p v-if="!workspace.native" class="notice" role="status">
      {{ $t("wakeSamples.desktopOnly") }}
    </p>
    <div class="form-grid">
      <SelectField
        id="wakeSampleLanguage"
        v-model="request.language"
        :label="$t('wakeSamples.language')"
        :disabled="active"
        :options="[
          { value: 'ru', label: 'Эй, Фоно' },
          { value: 'en', label: 'Hey Fono' },
        ]"
      />
      <SelectField
        id="wakeSampleCondition"
        v-model="request.condition"
        :label="$t('wakeSamples.condition')"
        :disabled="active"
        :options="[
          { value: 'quiet', label: $t('wakeSamples.quiet') },
          { value: 'noisy', label: $t('wakeSamples.noisy') },
        ]"
      />
    </div>
    <SelectField
      id="wakeSampleKind"
      v-model="request.kind"
      :label="$t('wakeSamples.kind')"
      :disabled="active"
      :options="[
        { value: 'phrase', label: $t('wakeSamples.phrase') },
        { value: 'other_speech', label: $t('wakeSamples.other') },
        { value: 'background', label: $t('wakeSamples.background') },
      ]"
    />
    <p class="notice">{{ prompt }}</p>
    <p class="muted">
      {{ $t("wakeSamples.progress", { count: selected.length, target }) }}
    </p>
    <WlCheckbox
      v-model="request.consent"
      :disabled="active || !workspace.native"
      >{{ $t("wakeSamples.consent") }}</WlCheckbox
    >
    <div class="actions">
      <WlButton
        variant="primary"
        :disabled="blocked || !request.consent || state.samples.length >= 200"
        @click="port.record(request)"
        >{{ $t("wakeSamples.record") }}</WlButton
      >
      <WlButton v-if="state.phase === 'recording'" @click="port.finish()">{{
        $t("wakeSamples.finish")
      }}</WlButton>
      <WlButton
        v-if="active"
        :disabled="state.phase === 'cancelling'"
        @click="port.cancel()"
        >{{ $t("wakeSamples.cancel") }}</WlButton
      >
    </div>
    <p v-if="active" role="status" aria-live="polite">
      {{
        state.phase === "recording"
          ? $t("wakeSamples.recording", {
              seconds: Math.ceil(state.remainingMs / 1000),
            })
          : $t(
              state.phase === "starting"
                ? "wakeSamples.starting"
                : state.phase === "saving"
                  ? "wakeSamples.saving"
                  : "wakeSamples.cancelling",
            )
      }}
    </p>
    <p v-if="state.error" class="error-text" role="alert">{{ state.error }}</p>
    <p class="muted">{{ $t("wakeSamples.reviewHint") }}</p>
    <ul v-if="selected.length" class="wake-sample-list">
      <li
        v-for="(sample, index) in selected"
        :key="sample.id"
        class="form-stack"
      >
        <div class="wake-sample-row">
          <span
            >{{ kindLabel(sample.kind) }} {{ selected.length - index }} ·
            {{ (sample.durationMs / 1000).toFixed(1) }}
            {{ $t("wakeSamples.seconds") }}</span
          >
          <div class="actions">
            <WlButton
              size="sm"
              :disabled="active"
              @click="port.listen(sample.id)"
              >{{ $t("wakeSamples.listen") }}</WlButton
            >
            <WlButton
              size="sm"
              :disabled="active"
              @click="port.remove(sample.id)"
              >{{ $t("wakeSamples.remove") }}</WlButton
            >
          </div>
        </div>
        <audio
          v-if="state.audio?.id === sample.id"
          :src="state.audio.url"
          controls
          autoplay
          :aria-label="$t('wakeSamples.playback')"
        />
      </li>
    </ul>
  </section>
</template>
<style scoped>
.wake-samples {
  border-top: 1px solid var(--fono-border);
  padding-top: var(--fono-space-5);
}
.wake-sample-list {
  list-style: none;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--fono-space-4);
}
.wake-sample-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: var(--fono-space-3);
}
audio {
  width: 100%;
  max-width: 100%;
}
</style>
