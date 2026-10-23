<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive } from "vue";
import { WlButton, WlCheckbox } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { message as $t } from "../../../shared/domain/messages";
import { captureSettingsLocked } from "../../../shared/domain/pretranscription";
import SelectField from "../../../shared/presentation/SelectField.vue";
import type { WakeSampleRequest } from "../domain/samples";
import {
  liraActive,
  wakeSampleSets,
  wakeSamplesInSet,
} from "../domain/samples";

const workspace = useWorkspace();
const port = workspace.wakeSamples;
const state = port.state;
const request = reactive<WakeSampleRequest>({
  phraseId: "privet_lira",
  language: "ru",
  kind: "phrase",
  condition: "quiet",
  consent: false,
});
const captureActive = computed(() => state.phase !== "idle");
const liveActive = computed(() => liraActive(state.live));
const active = computed(
  () =>
    captureActive.value ||
    state.testingId !== null ||
    liveActive.value ||
    state.livePending,
);
const dictationBusy = computed(() =>
  captureSettingsLocked(workspace.state.phase),
);
const blocked = computed(
  () => !workspace.native || active.value || dictationBusy.value,
);
const currentSet = computed(() =>
  wakeSampleSets.find(
    (set) =>
      set.phraseId === request.phraseId && set.language === request.language,
  )!,
);
const selectedSet = computed({
  get: () => currentSet.value.id,
  set: (id: string) => {
    const set = wakeSampleSets.find((set) => set.id === id);
    if (set) {
      request.phraseId = set.phraseId;
      request.language = set.language;
    }
  },
});
const phrase = computed(() => currentSet.value.phrase);
const selected = computed(() => wakeSamplesInSet(state.samples, request));
const target = computed(() =>
  request.kind === "phrase" ? 5 : request.kind === "other_speech" ? 3 : 1,
);
const prompt = computed(() =>
  request.kind === "phrase"
    ? $t("wakeSamples.sayPhrase", { phrase: phrase.value })
    : $t(
        request.kind === "other_speech"
          ? "wakeSamples.liraOtherPrompt"
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
    <section
      v-if="state.live.available"
      class="form-stack"
      :aria-label="$t('wakeLive.title')"
    >
      <h4>{{ $t("wakeLive.title") }}</h4>
      <p class="notice">{{ $t("wakeLive.hint") }}</p>
      <p role="status" aria-live="polite">
        {{ $t(`wakeLive.${state.live.phase}`) }}
      </p>
      <p v-if="state.live.message" class="error-text" role="alert">
        {{ state.live.message }}
      </p>
      <div class="actions">
        <WlButton
          v-if="!liveActive"
          variant="primary"
          :disabled="active || dictationBusy"
          @click="port.enableLive()"
          >{{ $t("wakeLive.enable") }}</WlButton
        >
        <WlButton
          v-else
          :disabled="state.livePending || state.live.phase === 'stopping'"
          @click="port.disableLive()"
          >{{ $t("wakeLive.disable") }}</WlButton
        >
      </div>
      <p v-if="liveActive" class="muted">{{ $t("wakeLive.samplesPaused") }}</p>
    </section>
    <p class="muted">{{ $t("wakeSamples.explanation") }}</p>
    <p v-if="state.testAvailable" class="notice">
      {{ $t("wakeSamples.testHint") }}
    </p>
    <p v-if="!workspace.native" class="notice" role="status">
      {{ $t("wakeSamples.desktopOnly") }}
    </p>
    <p v-else-if="dictationBusy" class="notice" role="status">
      {{ $t("wakeSamples.dictationBusy") }}
    </p>
    <div class="form-grid">
      <SelectField
        id="wakeSampleLanguage"
        v-model="selectedSet"
        :label="$t('wakeSamples.language')"
        :disabled="active"
        :options="
          wakeSampleSets.map((set) => ({
            value: set.id,
            label: set.archived
              ? $t('wakeSamples.archivedSet', { phrase: set.phrase })
              : set.phrase,
          }))
        "
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
    <p class="notice">
      {{ currentSet.archived ? $t("wakeSamples.archiveHint") : prompt }}
    </p>
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
        :disabled="
          blocked ||
          currentSet.archived ||
          !request.consent ||
          state.samples.length >= 200
        "
        @click="port.record(request)"
        >{{ $t("wakeSamples.record") }}</WlButton
      >
      <WlButton v-if="state.phase === 'recording'" @click="port.finish()">{{
        $t("wakeSamples.finish")
      }}</WlButton>
      <WlButton
        v-if="captureActive"
        :disabled="state.phase === 'cancelling'"
        @click="port.cancel()"
        >{{ $t("wakeSamples.cancel") }}</WlButton
      >
    </div>
    <p v-if="captureActive" role="status" aria-live="polite">
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
              v-if="state.testAvailable && !currentSet.archived"
              size="sm"
              :disabled="active || dictationBusy"
              @click="port.test(sample.id)"
              >{{ $t("wakeSamples.test") }}</WlButton
            >
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
        <p
          v-if="state.testingId === sample.id"
          role="status"
          aria-live="polite"
        >
          {{ $t("wakeSamples.testing") }}
        </p>
        <p v-if="state.testResults[sample.id]" role="status" aria-live="polite">
          {{
            $t(
              state.testResults[sample.id].detected
                ? "wakeSamples.detected"
                : "wakeSamples.notDetected",
            )
          }}
          ·
          {{
            $t("wakeSamples.testTime", {
              seconds: (
                state.testResults[sample.id].processingMs / 1000
              ).toFixed(1),
            })
          }}
        </p>
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
