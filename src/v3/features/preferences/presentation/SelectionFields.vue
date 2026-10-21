<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

import { WlButton, WlField, WlInput } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import PreferenceToggle from "./PreferenceToggle.vue";
const workspace = useWorkspace();
defineProps<{ active: boolean }>();
const draft = defineModel<Preferences>({ required: true });
</script>
<template>
  <section class="form-stack">
    <h3>{{ $t("ui.fea90688d9") }}</h3>
    <PreferenceToggle
      name="selectionCaptureEnabled"
      :disabled="active || workspace.state.triggerCaptureActive"
      :label="$t('ui.4cad5fa9b8')"
      :description="$t('ui.40d978d111')"
    />
    <WlField
      id="selectionHotkey"
      v-slot="field"
      :label="$t('ui.67bce1c66d')"
      :hint="$t('ui.d90cd6bfe9')"
    >
      <WlInput
        v-bind="field"
        v-model="draft.selectionHotkey"
        :disabled="active || workspace.state.triggerCaptureActive"
      />
    </WlField>
    <PreferenceToggle
      name="selectionClipboardFallback"
      :disabled="active || workspace.state.triggerCaptureActive"
      :label="$t('ui.deff4adbbe')"
      :description="$t('ui.fe9116db5c')"
    />
    <p class="muted">{{ $t("ui.265cfc3c5d") }}</p>
    <PreferenceToggle
      name="selectionAutoInsert"
      :disabled="active || workspace.state.triggerCaptureActive"
      :label="$t('ui.c38d3b5b34')"
      :description="$t('ui.555f12a379')"
    />
    <PreferenceToggle
      name="selectionHistoryEnabled"
      :label="$t('ui.814cc0735b')"
      :description="$t('ui.8565219416')"
    />
    <WlButton
      v-if="!workspace.native && workspace.selection.demo"
      @click="workspace.selection.demo()"
      >{{ $t("ui.0558fb623c") }}</WlButton
    >
    <p v-if="!workspace.native" class="notice">{{ $t("ui.153bf69efc") }}</p>
  </section>
</template>
