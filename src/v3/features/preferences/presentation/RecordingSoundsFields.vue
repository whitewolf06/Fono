<script setup lang="ts">
import { WlButton, WlSlider } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import { message as $t } from "../../../shared/domain/messages";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import PreferenceToggle from "./PreferenceToggle.vue";
const draft = defineModel<Preferences>({ required: true });
const workspace = useWorkspace();
const { run, busy, error } = useFeedback();
</script>
<template>
  <section class="form-stack">
    <PreferenceToggle
      id="recordingSoundsEnabled"
      name="recordingSoundsEnabled"
      :label="$t('recordingSounds.label')"
      :description="$t('recordingSounds.hint')"
    />
    <label id="recordingSoundsVolume">
      {{ $t("recordingSounds.volume") }} {{ draft.recordingSoundsVolume }}%
      <WlSlider
        v-model="draft.recordingSoundsVolume"
        :min="0"
        :max="100"
        :step="5"
        :disabled="!workspace.state.preferences.recordingSoundsEnabled"
        :aria-label="$t('recordingSounds.volume')"
      />
    </label>
    <div class="actions">
      <WlButton
        size="sm"
        :loading="busy"
        :disabled="
          !workspace.native ||
          !workspace.state.preferences.recordingSoundsEnabled ||
          draft.recordingSoundsVolume === 0
        "
        @click="
          run(() =>
            workspace.settings.previewRecordingSounds(
              draft.recordingSoundsVolume,
            ),
          )
        "
        >{{ $t("recordingSounds.preview") }}</WlButton
      >
    </div>
    <p class="muted">
      {{
        $t(workspace.native ? "recordingSounds.ready" : "recordingSounds.demo")
      }}
    </p>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
  </section>
</template>
