<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

import { computed, watch } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import { microphones, languages, accelerations } from "../domain/preferences";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import SelectField from "../../../shared/presentation/SelectField.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import SettingsDisclosure from "../../../shared/presentation/SettingsDisclosure.vue";
import GpuMemoryField from "./GpuMemoryField.vue";
import DecodingFields from "./DecodingFields.vue";
import { compatibleModelPreferences } from "../../../shared/domain/speechModels";
const props = defineProps<{ scope?: "microphone" | "recognition" }>();
const draft = defineModel<Preferences>({ required: true });
const workspace = useWorkspace();
const { run, busy, error } = useFeedback();
const selected = computed(() =>
  workspace.state.models.find((m) => m.id === draft.value.model),
);
const offline = computed(
  () => selected.value?.engine && selected.value.engine !== "whisper",
);
const languageOptions = computed(() =>
  offline.value
    ? languages.filter(
        (l) =>
          l.value === "auto" ||
          (selected.value?.languageSelection &&
            selected.value.languages?.includes(l.value)),
      )
    : languages,
);
const accelerationOptions = computed(() =>
  (workspace.state.accelerations || accelerations).filter(
    (a) => !offline.value || ["auto", "cpu"].includes(a.value),
  ),
);
watch(
  () => draft.value.model,
  () =>
    Object.assign(
      draft.value,
      compatibleModelPreferences(selected.value, draft.value),
    ),
);
const installed = computed(() =>
  workspace.state.models
    .filter((m) => m.status === "installed")
    .map((m) => ({ value: m.id, label: m.name })),
);
</script>
<template>
  <div class="form-stack">
    <template v-if="props.scope !== 'recognition'">
      <SelectField
        id="microphone"
        v-model="draft.microphone"
        :label="$t('ui.ebb14dc507')"
        :options="workspace.state.devices || microphones"
        :hint="$t('ui.ee345bd502')"
      />
      <div
        v-if="!workspace.state.microphoneAvailable"
        class="notice error"
        role="alert"
      >
        {{ $t("ui.9010565737") }}
      </div>
      <div class="signal-test">
        <span
          >{{ $t("ui.eb8775eff9") }}
          <small v-if="!workspace.native">{{
            $t("ui.8de2747e9e")
          }}</small></span
        >
        <div
          class="signal-meter"
          role="meter"
          :aria-label="$t('ui.eb8775eff9')"
          :aria-valuenow="Math.round(workspace.state.testSignal * 100)"
          aria-valuemin="0"
          aria-valuemax="100"
        >
          <span :style="{ width: workspace.state.testSignal * 100 + '%' }" />
        </div>
      </div>
      <div class="actions">
        <WlButton
          size="sm"
          :loading="busy"
          @click="
            run(
              () => workspace.settings.testMicrophone(draft.microphone),
              workspace.native ? $t('ui.0f27d53461') : $t('ui.0921976316'),
            )
          "
          ><template #icon><AppIcon name="microphone" /></template
          >{{ $t("ui.cd1333f138") }}</WlButton
        ><WlButton
          size="sm"
          variant="ghost"
          :disabled="busy"
          @click="run(() => workspace.settings.playSample())"
          ><template #icon><AppIcon name="play" /></template
          >{{ $t("ui.b6e263a751") }}</WlButton
        >
      </div>
      <small v-if="!workspace.native" class="muted">{{
        $t("ui.03ae4cb669")
      }}</small>
    </template>
    <template v-if="props.scope !== 'microphone'">
      <SelectField
        id="model"
        v-model="draft.model"
        :label="$t('ui.8b9baf7846')"
        :options="installed"
        :hint="installed.length ? $t('ui.619d6b13d9') : $t('ui.44364b2632')"
      />
      <SelectField
        id="language"
        v-model="draft.language"
        :label="$t('ui.5bd57038db')"
        :options="languageOptions"
      />
      <slot name="models" />
      <SettingsDisclosure id="audio-advanced">
        <SelectField
          id="acceleration"
          v-model="draft.acceleration"
          :label="$t('ui.c2ccfcfe69')"
          :options="accelerationOptions"
        />
        <p v-if="offline" class="muted">{{ $t("ui.fd0519ddd2") }}</p>
        <GpuMemoryField v-if="!offline" v-model="draft" />
        <DecodingFields v-if="!offline" v-model="draft" />
        <slot name="advanced" />
      </SettingsDisclosure>
    </template>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
  </div>
</template>
