<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed, ref, watch } from "vue";
import { WlButton, WlField, WlTextarea } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import SelectionDelivery from "./SelectionDelivery.vue";
const workspace = useWorkspace();
const current = computed(() => workspace.selection.state.current);
const edit = computed(() => current.value?.edit);
const instruction = ref("");
watch(
  [() => current.value?.operationId, () => edit.value?.instruction],
  () => {
    instruction.value = edit.value?.instruction ?? "";
  },
  { immediate: true },
);
const active = computed(
  () =>
    Boolean(edit.value?.operationId) ||
    ["replacing", "copying", "cancelling"].includes(
      current.value?.delivery?.phase ?? "idle",
    ),
);
const { run, busy, error } = useFeedback();
function start() {
  const id = current.value?.operationId;
  if (id) void run(() => workspace.selection.startInstruction(id));
}
function stop() {
  const id = current.value?.operationId,
    job = edit.value?.operationId;
  if (id && job) void run(() => workspace.selection.stopInstruction(id, job));
}
function cancel() {
  const id = current.value?.operationId,
    job = edit.value?.operationId;
  if (id && job) void run(() => workspace.selection.cancelEdit(id, job));
}
function transform() {
  const id = current.value?.operationId;
  if (id)
    void run(() => workspace.selection.transform(id, instruction.value, true));
}
const status = computed(
  () =>
    ({
      get starting() {
        return uiMessage("ui.a64c4eb601");
      },
      get recording() {
        return uiMessage("ui.669c300bce");
      },
      get recognizing() {
        return uiMessage("ui.276687e0d8");
      },
      get processing() {
        return uiMessage("ui.b076c3cf48");
      },
      get cancelling() {
        return uiMessage("ui.1172eb60e0");
      },
      idle: "",
      ready: "",
    })[edit.value?.phase ?? "idle"],
);
</script>
<template>
  <section class="form-stack">
    <WlField
      id="selectionInstruction"
      v-slot="field"
      :label="$t('ui.d10dc0fb12')"
    >
      <WlTextarea
        v-bind="field"
        v-model="instruction"
        :disabled="active || busy"
        :rows="3"
        :maxlength="4096"
        :placeholder="$t('ui.c1440b8682')"
      />
    </WlField>
    <p class="muted">{{ $t("ui.e1d4da83f3") }}</p>
    <div class="actions">
      <WlButton
        :disabled="active || busy"
        @click="instruction = $t('ui.18ee964f5e')"
        >{{ $t("ui.255b4cb36d") }}</WlButton
      >
      <WlButton
        :disabled="active || busy"
        @click="instruction = $t('ui.5028e09d99')"
        >{{ $t("ui.5fdbe8a0c7") }}</WlButton
      >
      <WlButton
        :disabled="active || busy"
        @click="instruction = $t('ui.4c58add8d2')"
        >{{ $t("ui.f891bfb228") }}</WlButton
      >
    </div>
    <div class="actions">
      <WlButton
        v-if="edit?.phase === 'recording' || edit?.phase === 'starting'"
        :loading="busy"
        @click="stop"
        >{{ $t("ui.4d8004276a") }}</WlButton
      >
      <WlButton v-else :disabled="active || busy" @click="start">{{
        workspace.native ? $t("ui.1253514388") : $t("ui.6d253dec28")
      }}</WlButton>
      <WlButton
        v-if="active"
        :disabled="edit?.phase === 'cancelling' || busy"
        @click="cancel"
        >{{ $t("ui.b9b86803ad") }}</WlButton
      >
    </div>
    <p v-if="status" role="status">{{ status }}</p>
    <p v-if="workspace.native" class="notice">{{ $t("ui.4961516cf1") }}</p>
    <p v-else class="notice">{{ $t("ui.8d7e6f5795") }}</p>
    <WlButton
      :disabled="active || busy || !instruction.trim()"
      @click="transform"
      >{{
        workspace.native ? $t("ui.5ff9b5cdfa") : $t("ui.134885fa4f")
      }}</WlButton
    >
    <p v-if="error || edit?.message" role="alert" class="error-text">
      {{ error || edit?.message }}
    </p>
    <template v-if="edit?.result">
      <h3>{{ $t("ui.c5b4b71005") }}</h3>
      <WlTextarea
        :model-value="edit.result"
        readonly
        :rows="6"
        :aria-label="$t('ui.51f4c953d9')"
      />
      <p class="muted">
        {{ $t("ui.5db79f6718") }}
        {{ current?.autoInsert ? $t("ui.9ce6cfd4c1") : $t("ui.c779212b27") }}
      </p>
      <SelectionDelivery
        :instruction-matches="instruction === edit.instruction"
      />
    </template>
  </section>
</template>
