<script setup lang="ts">
import { uiIntlLocale } from "../../../shared/domain/locale";

import { message as $t } from "../../../shared/domain/messages";

import { WlButton, WlField, WlTextarea } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import { useProcessingPreview } from "../application/useProcessingPreview";
const draft = defineModel<Preferences>({ required: true });
const {
  workspace,
  input,
  original,
  result,
  busy,
  capturing,
  captureBusy,
  error,
  notice,
  usesModel,
  connectionChanged,
  connectionMissing,
  activeDictation,
  canTest,
  test,
  useLast,
  startCapture,
  finishCapture,
  cancelCapture,
  copy,
} = useProcessingPreview(draft);
</script>
<template>
  <section class="processing-preview" aria-labelledby="processing-test-heading">
    <header>
      <h3 id="processing-test-heading">{{ $t("ui.263e4ff6fe") }}</h3>
      <p class="muted">{{ $t("ui.7c35b7ce22") }}</p>
    </header>
    <p v-if="!workspace.native" class="notice">{{ $t("ui.f556a242bf") }}</p>
    <WlField v-slot="field" :label="$t('ui.290773056a')"
      ><WlTextarea
        v-bind="field"
        v-model="input"
        :rows="4"
        :disabled="capturing || captureBusy || busy"
        :placeholder="$t('ui.a944bff7de')"
    /></WlField>
    <div class="processing-preview-actions">
      <WlButton
        size="sm"
        :disabled="
          capturing ||
          captureBusy ||
          busy ||
          (!workspace.state.last.entry && !workspace.state.last.draft)
        "
        @click="useLast"
        ><AppIcon name="clock" :size="15" />{{ $t("ui.2b57744e17") }}</WlButton
      >
      <template v-if="capturing"
        ><WlButton size="sm" :loading="captureBusy" @click="finishCapture"
          ><AppIcon name="check" :size="15" />{{
            $t("ui.54fe059040")
          }}</WlButton
        ><WlButton size="sm" :disabled="captureBusy" @click="cancelCapture">{{
          $t("ui.7c47f7292b")
        }}</WlButton></template
      >
      <WlButton
        v-else
        size="sm"
        :loading="captureBusy"
        :disabled="busy || activeDictation"
        @click="startCapture"
        ><AppIcon name="microphone" :size="15" />{{
          workspace.native ? $t("ui.968db772ec") : $t("ui.0d33aa8fea")
        }}</WlButton
      >
      <WlButton
        variant="primary"
        size="sm"
        :loading="busy"
        :disabled="!canTest"
        @click="test"
        ><AppIcon name="sparkle" :size="15" />{{
          $t("ui.6f3fb92f4a")
        }}</WlButton
      >
    </div>
    <p v-if="capturing" class="notice" role="status">
      {{ $t("ui.b953340507") }}
    </p>
    <p v-if="connectionChanged" class="notice">{{ $t("ui.f32b92db80") }}</p>
    <p v-else-if="connectionMissing" class="notice">
      {{ $t("ui.44a26bd9aa") }}
    </p>
    <p v-else-if="usesModel" class="muted">
      {{ $t("ui.ccf9ad4514") }}
      {{ workspace.state.preferences.processingModel }}
      {{ $t("ui.c69f2b3683") }}
    </p>
    <p v-else class="muted">{{ $t("ui.795e3ee961") }}</p>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    <p v-if="notice" class="notice" role="status">{{ notice }}</p>
    <section
      v-if="result"
      class="processing-preview-output"
      :aria-label="$t('ui.02c87b9374')"
    >
      <div>
        <strong>{{ $t("ui.290773056a") }}</strong>
        <pre>{{ original }}</pre>
      </div>
      <div>
        <header>
          <strong>{{ $t("ui.71b415a72b") }}</strong
          ><WlButton size="sm" @click="copy"
            ><AppIcon name="copy" :size="15" />{{
              $t("ui.1ecddf40bf")
            }}</WlButton
          >
        </header>
        <pre>{{ result.text }}</pre>
      </div>
      <small
        >{{ result.model || $t("ui.574e06000b") }} ·
        {{
          (result.elapsedMs / 1000).toLocaleString(uiIntlLocale(), {
            maximumFractionDigits: 2,
          })
        }}
        {{ $t("ui.a1f5d39a12") }}</small
      >
    </section>
  </section>
</template>
