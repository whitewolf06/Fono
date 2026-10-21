<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed, ref } from "vue";
import { WlButton, WlField, WlInput } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import { triggerConflict } from "../../../shared/domain/triggers";
import { useWorkspace } from "../../../shared/application/workspace";
import { useTriggerCapture } from "../application/useTriggerCapture";
import SelectField from "../../../shared/presentation/SelectField.vue";
const props = defineProps<{
  scope?: "hotkey" | "command-hotkey";
  active: boolean;
}>();
const draft = defineModel<Preferences>({ required: true });
const autoStopOptions = computed(() => [
  { value: 0, label: uiMessage("autoStop.off") },
  ...[2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000, 10000]
    .concat(
      draft.value.toggleAutoStopMs > 0 ? [draft.value.toggleAutoStopMs] : [],
    )
    .filter((value, index, values) => values.indexOf(value) === index)
    .sort((a, b) => a - b)
    .map((value) => ({
      value,
      label: uiMessage("autoStop.seconds", { seconds: value / 1000 }),
    })),
]);
const workspace = useWorkspace();
const target = ref<"dictation" | "command">("dictation");
const captureError = ref("");
const { capturing, message, start, cancel } = useTriggerCapture((candidate) => {
  captureError.value = "";
  if (target.value === "command") {
    if (candidate.trigger !== "shortcut" || !candidate.hotkey) {
      captureError.value = uiMessage("ui.8d363d872b");
      return;
    }
    draft.value.commandHotkey = candidate.hotkey;
  } else {
    draft.value.dictationTrigger = candidate.trigger;
    if (candidate.hotkey) draft.value.hotkey = candidate.hotkey;
  }
});
const conflict = computed(() => triggerConflict(draft.value));
const disabled = computed(() => props.active || capturing.value);
const keyboardHook = computed(() =>
  ["right_alt", "right_ctrl", "caps_lock"].includes(
    draft.value.dictationTrigger,
  ),
);
const behavior = computed(() =>
  draft.value.hotkeyMode === "hold"
    ? uiMessage("ui.3c5f6bcb70")
    : keyboardHook.value
      ? uiMessage("ui.9e82172335")
      : uiMessage("ui.9e3088266e"),
);
function choose(value: "dictation" | "command") {
  target.value = value;
  captureError.value = "";
  void start();
}
</script>
<template>
  <div class="form-stack">
    <template v-if="scope !== 'command-hotkey'">
      <SelectField
        id="dictationTrigger"
        v-model="draft.dictationTrigger"
        :label="$t('ui.520fadb90f')"
        :disabled="disabled"
        :options="[
          {
            value: 'shortcut',
            get label() {
              return $t('ui.2c2b46b6e3');
            },
          },
          {
            value: 'right_alt',
            get label() {
              return $t('ui.3424783930');
            },
          },
          {
            value: 'right_ctrl',
            get label() {
              return $t('ui.671c2fb9e5');
            },
          },
          { value: 'caps_lock', label: 'CapsLock' },
          {
            value: 'x_button1',
            get label() {
              return $t('ui.75a016d827');
            },
          },
          {
            value: 'x_button2',
            get label() {
              return $t('ui.9d9f714474');
            },
          },
        ]"
      />
      <WlField
        v-if="draft.dictationTrigger === 'shortcut'"
        id="hotkey"
        v-slot="field"
        :label="$t('ui.18fb8ccf7e')"
        :hint="behavior"
      >
        <WlInput v-bind="field" v-model="draft.hotkey" :disabled="disabled" />
      </WlField>
      <p v-else class="muted">{{ behavior }}</p>
      <WlButton :disabled="disabled" @click="choose('dictation')">{{
        $t("ui.26811b79a8")
      }}</WlButton>
      <SelectField
        id="hotkeyMode"
        v-model="draft.hotkeyMode"
        :label="$t('ui.840387c669')"
        :disabled="disabled"
        :options="[
          {
            value: 'hold',
            get label() {
              return $t('ui.8722a38dbd');
            },
          },
          {
            value: 'toggle',
            label: keyboardHook ? $t('ui.48cca8005a') : $t('ui.1bd81f444d'),
          },
        ]"
        :hint="$t('ui.8239145a0b')"
      />
      <SelectField
        v-if="draft.hotkeyMode === 'toggle'"
        id="toggleAutoStopMs"
        v-model="draft.toggleAutoStopMs"
        :label="$t('autoStop.label')"
        :options="autoStopOptions"
        :hint="$t('autoStop.hint')"
        :disabled="disabled"
      />
      <p v-if="draft.dictationTrigger === 'right_alt'" class="notice">
        {{ $t("ui.3d18ea8003") }}
      </p>
      <p v-if="draft.dictationTrigger === 'caps_lock'" class="notice">
        {{ $t("ui.8201c766e9") }}
      </p>
      <p v-if="draft.dictationTrigger.startsWith('x_button')" class="notice">
        {{ $t("ui.4e167cdd83") }}
      </p>
    </template>
    <WlField
      id="commandHotkey"
      v-slot="field"
      :label="$t('ui.08783cdee7')"
      :hint="$t('ui.e08944ce3a')"
    >
      <WlInput
        v-bind="field"
        v-model="draft.commandHotkey"
        :disabled="disabled"
      />
    </WlField>
    <WlButton :disabled="disabled" @click="choose('command')">{{
      $t("ui.81982ed781")
    }}</WlButton>
    <p v-if="capturing" class="notice" role="status">{{ message }}</p>
    <WlButton v-if="capturing" @click="cancel">{{
      $t("ui.0ebe44a248")
    }}</WlButton>
    <p v-else-if="message" class="muted" role="status">{{ message }}</p>
    <p v-if="conflict || captureError" class="error-text" role="alert">
      {{ captureError || conflict }}
    </p>
    <p v-if="active" class="notice">{{ $t("ui.e1ecf36dda") }}</p>
    <p class="muted">{{ $t("ui.8d7ff1d12f") }}</p>
    <details v-if="scope !== 'command-hotkey'" class="advanced">
      <summary>{{ $t("ui.d0fc417c8f") }}</summary>
      <p class="muted">
        {{ $t("ui.b43b98db76") }} <code>Fono.exe --start</code>,
        <code>--stop</code>, <code>--toggle</code> {{ $t("ui.bcacbff7a5") }}
        <code>--cancel</code>{{ $t("ui.2a35056c91") }}
      </p>
    </details>
    <p v-if="!workspace.native" class="notice">{{ $t("ui.5fecf677e2") }}</p>
  </div>
</template>
