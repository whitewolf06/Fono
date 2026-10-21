<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed, ref, watch } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import type { SelectionCapturePort } from "../domain/selection";
import { useFeedback } from "../../../shared/application/feedback";
import { cancelSelection } from "../application/cancelSelection";
const props = defineProps<{
  port: SelectionCapturePort;
  preferences: Preferences;
  native: boolean;
}>();
const current = computed(() => props.port.state.current);
const edit = computed(() => current.value?.edit);
const delivery = computed(() => current.value?.delivery);
const instruction = ref("");
watch(
  [() => current.value?.operationId, () => edit.value?.instruction],
  () => {
    instruction.value = edit.value?.instruction ?? "";
  },
  { immediate: true },
);
const { run, busy, error } = useFeedback();
const working = computed(
  () =>
    !!edit.value?.operationId ||
    ["replacing", "copying", "cancelling"].includes(
      delivery.value?.phase ?? "idle",
    ),
);
function action(
  kind: "record" | "stop" | "transform" | "insert" | "copy" | "cancel",
) {
  const s = current.value;
  if (!s) return;
  void run(async () => {
    if (kind === "cancel") return cancelSelection(props.port);
    if (kind === "record") return props.port.startInstruction(s.operationId);
    if (kind === "stop" && edit.value?.operationId)
      return props.port.stopInstruction(s.operationId, edit.value.operationId);
    if (kind === "transform")
      return props.port.transform(s.operationId, instruction.value, true);
    const result = s.edit?.resultId;
    if (!result) return;
    if (kind === "insert")
      return props.port.insertResult(s.operationId, result);
    if (kind === "copy") return props.port.copyResult(s.operationId, result);
  });
}
const status = computed(
  () =>
    ({
      get starting() {
        return uiMessage("ui.a64c4eb601");
      },
      get recording() {
        return uiMessage("ui.ab3c96a47f");
      },
      get recognizing() {
        return uiMessage("ui.276687e0d8");
      },
      get processing() {
        return uiMessage("ui.3f9ea95c04");
      },
      get cancelling() {
        return uiMessage("ui.2cbe530e97");
      },
      idle: "",
      ready: "",
    })[edit.value?.phase ?? "idle"],
);
</script>
<template>
  <div v-if="current" class="overlay-preview-wrap">
    <section
      class="overlay-indicator overlay-preview selection-overlay"
      :aria-label="$t('ui.577aa758f2')"
      data-overlay-interactive
      :style="{
        zoom: preferences.overlayScale / 100,
        '--fono-overlay-zoom': preferences.overlayScale / 100,
        opacity: preferences.overlayOpacity / 100,
      }"
    >
      <div class="section-header">
        <h3>{{ $t("ui.577aa758f2") }}</h3>
        <WlButton :disabled="busy" size="sm" @click="action('cancel')">{{
          $t("ui.8fbe9b75cb")
        }}</WlButton>
      </div>
      <p v-if="current.phase === 'reading'" role="status">
        {{ $t("ui.2b26e0fc63") }}
      </p>
      <p v-else-if="current.phase === 'error'" role="alert">
        {{ current.message }}
      </p>
      <template v-else>
        <details>
          <summary>{{ $t("ui.44a2b1f65a") }}</summary>
          <p class="selection-overlay-text">{{ current.text }}</p>
        </details>
        <p class="selection-overlay-text" :aria-label="$t('ui.d10dc0fb12')">
          <strong>{{ $t("ui.3420df3736") }}</strong>
          {{ instruction || $t("ui.4614f88f8a") }}
        </p>
        <div class="actions">
          <WlButton
            size="sm"
            :disabled="working || busy"
            @click="instruction = $t('ui.18ee964f5e')"
            >{{ $t("ui.255b4cb36d") }}</WlButton
          >
          <WlButton
            size="sm"
            :disabled="working || busy"
            @click="instruction = $t('ui.5028e09d99')"
            >{{ $t("ui.5fdbe8a0c7") }}</WlButton
          >
          <WlButton
            size="sm"
            :disabled="working || busy"
            @click="instruction = $t('ui.4c58add8d2')"
            >{{ $t("ui.f891bfb228") }}</WlButton
          >
        </div>
        <div class="actions">
          <WlButton
            v-if="edit?.phase === 'starting' || edit?.phase === 'recording'"
            :disabled="busy"
            @click="action('stop')"
            >{{ $t("ui.4d8004276a") }}</WlButton
          >
          <WlButton
            v-else
            :disabled="working || busy"
            @click="action('record')"
            >{{ native ? $t("ui.1253514388") : $t("ui.6d253dec28") }}</WlButton
          >
          <WlButton
            :disabled="working || busy || !instruction.trim()"
            @click="action('transform')"
            >{{ native ? $t("ui.5ff9b5cdfa") : $t("ui.134885fa4f") }}</WlButton
          >
        </div>
        <p v-if="status" role="status">{{ status }}</p>
        <p class="muted">
          {{ native ? $t("ui.384eaf639a") : $t("ui.fded00f19d") }}
        </p>
        <p class="muted">
          {{ current.autoInsert ? $t("ui.ac98858132") : $t("ui.9e15694d10") }}
          {{
            current.historyEnabled ? $t("ui.88cdd4ed49") : $t("ui.21dc26dc9d")
          }}
        </p>
        <template v-if="edit?.result">
          <p class="selection-overlay-text" :aria-label="$t('ui.07e3acdea5')">
            {{ edit.result }}
          </p>
          <div class="actions">
            <WlButton
              :disabled="
                working ||
                busy ||
                !delivery?.replaceAllowed ||
                instruction !== edit.instruction
              "
              @click="action('insert')"
              >{{ $t("ui.ec79f4d0c8") }}</WlButton
            >
            <WlButton :disabled="working || busy" @click="action('copy')">{{
              $t("ui.1ecddf40bf")
            }}</WlButton>
          </div>
          <p v-if="delivery?.phase === 'submitted'" role="status">
            {{ native ? $t("ui.4652589ada") : $t("ui.368b41bb78") }}
          </p>
          <p v-if="delivery?.phase === 'copied'" role="status">
            {{ native ? $t("ui.f3a941e04d") : $t("ui.045d4aa932") }}
          </p>
          <p
            v-if="['blocked', 'uncertain'].includes(delivery?.phase ?? '')"
            role="alert"
          >
            {{ $t("ui.c44194d64f") }}
          </p>
          <p
            v-if="
              ['replacing', 'copying', 'cancelling'].includes(
                delivery?.phase ?? '',
              )
            "
            role="status"
          >
            {{ $t("ui.ccb10f5082") }}
          </p>
        </template>
      </template>
      <p
        v-if="error || edit?.message || delivery?.message"
        role="alert"
        class="error-text"
      >
        {{ error || edit?.message || delivery?.message }}
      </p>
      <p v-if="current.historyMessage" role="alert" class="error-text">
        {{ current.historyMessage }}
      </p>
    </section>
  </div>
</template>
