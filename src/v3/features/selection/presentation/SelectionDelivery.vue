<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

import { computed } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
defineProps<{ instructionMatches: boolean }>();
const workspace = useWorkspace();
const current = computed(() => workspace.selection.state.current);
const delivery = computed(() => current.value?.delivery);
const active = computed(() =>
  ["replacing", "copying", "cancelling"].includes(
    delivery.value?.phase ?? "idle",
  ),
);
const { run, busy, error } = useFeedback();
function action(kind: "arm" | "copy" | "cancel") {
  const id = current.value?.operationId,
    result = current.value?.edit?.resultId;
  if (!id || !result) return;
  void run(() =>
    kind === "arm"
      ? workspace.selection.armReplacement(id, result)
      : kind === "copy"
        ? workspace.selection.copyResult(id, result)
        : workspace.selection.cancelDelivery(id, result),
  );
}
function demo() {
  if (workspace.selection.demoReplace)
    void run(workspace.selection.demoReplace);
}
</script>
<template>
  <section v-if="current?.edit?.resultId" class="form-stack">
    <div class="actions">
      <WlButton
        :disabled="
          busy ||
          active ||
          !delivery?.replaceAllowed ||
          !instructionMatches ||
          Boolean(current.edit.operationId)
        "
        @click="action('arm')"
        >{{ $t("ui.f2418b9058") }}</WlButton
      >
      <WlButton
        :disabled="busy || active || Boolean(current.edit.operationId)"
        @click="action('copy')"
        >{{ $t("ui.cd4665b319") }}</WlButton
      >
      <WlButton
        v-if="delivery?.phase === 'armed' || active"
        :disabled="busy || delivery?.phase === 'cancelling'"
        @click="action('cancel')"
        >{{ $t("ui.69057dd1cc") }}</WlButton
      >
    </div>
    <p v-if="delivery?.phase === 'armed'" role="status" class="notice">
      {{ $t("ui.8c39b360f8") }} {{ workspace.state.preferences.selectionHotkey
      }}{{ $t("ui.4cae9853a1") }}
    </p>
    <WlButton
      v-if="
        !workspace.native &&
        workspace.selection.demoReplace &&
        delivery?.phase === 'armed'
      "
      @click="demo"
      >{{ $t("ui.712033c945") }}</WlButton
    >
    <p
      v-if="delivery?.phase === 'replacing' || delivery?.phase === 'cancelling'"
      role="status"
    >
      {{ $t("ui.72c9e3ab5c") }}
    </p>
    <p v-if="delivery?.phase === 'copying'" role="status">
      {{ $t("ui.2e6688c0ac") }}
    </p>
    <p v-if="delivery?.phase === 'submitted'" role="status">
      {{ workspace.native ? $t("ui.80c21c8a38") : $t("ui.368b41bb78") }}
    </p>
    <p v-if="delivery?.phase === 'copied'" role="status">
      {{ workspace.native ? $t("ui.f3a941e04d") : $t("ui.29adedc09d") }}
    </p>
    <p
      v-if="delivery?.phase === 'blocked' || delivery?.phase === 'uncertain'"
      role="alert"
      class="notice"
    >
      {{ $t("ui.b7e5f64fb3") }}
    </p>
    <p v-if="delivery?.message || error" role="alert" class="error-text">
      {{ error || delivery?.message }}
    </p>
  </section>
</template>
