<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed, nextTick, ref } from "vue";
import { WlSwitch } from "@whitelife-core/ui-kit";
import type { ToggleKey } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
import { useFeedback } from "../../../shared/application/feedback";
import {
  WAKE_WORD_AVAILABLE,
  unavailableWakeMessage,
} from "../../../shared/domain/wakeAvailability";
const props = withDefaults(
  defineProps<{
    name: ToggleKey;
    label: string;
    description?: string;
    compact?: boolean;
    disabled?: boolean;
    effectiveValue?: boolean;
  }>(),
  { effectiveValue: undefined },
);
const workspace = useWorkspace();
const ui = useInteraction();
const { run } = useFeedback();
const changing = ref(false);
const currentValue = computed(() =>
  props.name === "wakeEnabled" && !WAKE_WORD_AVAILABLE
    ? false
    : (props.effectiveValue ?? workspace.state.preferences[props.name]),
);
const unavailable = computed(
  () => props.name === "wakeEnabled" && !WAKE_WORD_AVAILABLE,
);
const disabled = computed(() => props.disabled || unavailable.value);
const pending = computed(
  () => changing.value || workspace.state.pending[props.name],
);
let nativeInput: HTMLInputElement | undefined;
let restoreFocus = false;
function rememberInput(event: FocusEvent) {
  nativeInput = event.currentTarget as HTMLInputElement;
}
function syncNativeChecked(event: Event) {
  // A refused controlled change does not update the model. Restore the native
  // checkbox immediately, keeping the same element and its keyboard focus.
  nativeInput = event.currentTarget as HTMLInputElement;
  restoreFocus ||= nativeInput.ownerDocument.activeElement === nativeInput;
  nativeInput.checked = currentValue.value;
}
async function change(value: boolean) {
  if (disabled.value || pending.value) return;
  // Browser callbacks can flush Vue between the model listener and @change.
  // Capture focus before disabling, rather than after the native input blurs.
  restoreFocus =
    !!nativeInput && nativeInput.ownerDocument.activeElement === nativeInput;
  changing.value = true;
  try {
    await applyChange(value);
  } finally {
    changing.value = false;
    await nextTick();
    const document = nativeInput?.ownerDocument;
    if (
      restoreFocus &&
      nativeInput?.isConnected &&
      !nativeInput.disabled &&
      document?.activeElement === document?.body
    )
      nativeInput.focus({ preventScroll: true });
    restoreFocus = false;
  }
}
async function applyChange(value: boolean) {
  if (
    props.name === "wakeEnabled" &&
    value &&
    workspace.state.wakeSetup &&
    !workspace.state.wakeSetup.verified
  ) {
    await run(() => {
      throw new Error(uiMessage("ui.e3bf1feef8"));
    });
    return;
  }
  if (
    props.name === "trainerEnabled" &&
    value &&
    !workspace.state.preferences.analyticsConsent
  ) {
    if (
      !(await ui.confirm({
        get title() {
          return uiMessage("ui.6e1ad9eea0");
        },
        text:
          uiMessage("ui.b0c7824902") +
          (workspace.native ? "" : uiMessage("ui.05c147946e")),
        get accept() {
          return uiMessage("ui.94d1d823b9");
        },
      }))
    )
      return;
    workspace.state.pending.trainerEnabled = true;
    await run(() =>
      workspace.settings.save({ analyticsConsent: true, trainerEnabled: true }),
    );
    workspace.state.pending.trainerEnabled = false;
  } else if (props.name === "cloudConsent" && !value) {
    await run(() =>
      workspace.settings.save({ cloudConsent: false, trainerAiEnabled: false }),
    );
  } else if (props.name === "analyticsConsent" && !value) {
    if (
      !(await ui.confirm({
        get title() {
          return uiMessage("ui.9aaa58fd95");
        },
        text: workspace.native
          ? uiMessage("ui.e6832c3e99")
          : uiMessage("ui.db995c2f4b"),
        get accept() {
          return uiMessage("ui.bb5bdd176c");
        },
      }))
    )
      return;
    await run(() =>
      workspace.settings.save({
        analyticsConsent: false,
        trainerEnabled: false,
        trainerAiEnabled: false,
      }),
    );
  } else await run(() => workspace.settings.toggle(props.name, value));
}
</script>
<template>
  <div
    class="toggle-row"
    :class="{ 'is-compact': compact }"
    :aria-busy="pending || undefined"
  >
    <div v-if="!compact">
      <strong>{{ label }}</strong>
      <p v-if="description || unavailable">
        {{ unavailable ? unavailableWakeMessage() : description }}
      </p>
    </div>
    <WlSwitch
      :model-value="currentValue"
      :disabled="disabled || pending"
      :title="unavailable ? unavailableWakeMessage() : undefined"
      :aria-label="label"
      @update:model-value="change"
      @change="syncNativeChecked"
      @focus="rememberInput"
    />
    <span v-if="pending" class="sr-only" role="status">{{
      $t("ui.a77d5c4ecc")
    }}</span>
  </div>
</template>
