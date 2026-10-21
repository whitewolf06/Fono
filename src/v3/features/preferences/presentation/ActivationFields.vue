<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed, defineAsyncComponent } from "vue";
import { WlField, WlInput, WlSlider } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import type { Preferences } from "../../../shared/domain/contracts";
import PreferenceToggle from "./PreferenceToggle.vue";
import TriggerFields from "./TriggerFields.vue";
import SelectionFields from "./SelectionFields.vue";
import SelectField from "../../../shared/presentation/SelectField.vue";
import { LIVE_DICTATION_ENABLED } from "../../../shared/domain/dictationMode";
import {
  WAKE_WORD_AVAILABLE,
  unavailableWakeMessage,
  alternativeWakeMessage,
} from "../../../shared/domain/wakeAvailability";
const wakeCompiled =
  typeof __FONO_WAKE_WORD__ !== "undefined" && __FONO_WAKE_WORD__;
const NativeWakeSetup =
  typeof __FONO_WAKE_WORD__ !== "undefined" && __FONO_WAKE_WORD__
    ? defineAsyncComponent(() => import("./NativeWakeSetup.vue"))
    : null;
defineProps<{
  scope?: "wake" | "hotkey" | "command-hotkey";
  advanced?: boolean;
}>();
const workspace = useWorkspace();
const draft = defineModel<Preferences>({ required: true });
const active = computed(() =>
  [
    "listening",
    "silence",
    "transcribing",
    "processing",
    "awaiting_action",
  ].includes(workspace.state.phase),
);
const wakeLanguages = computed(
  () =>
    workspace.state.wakeCapabilities?.languages || [
      {
        value: "ru",
        get label() {
          return uiMessage("ui.6da140fdd4");
        },
      },
      {
        value: "en",
        get label() {
          return uiMessage("ui.e7c1ef1d8e");
        },
      },
    ],
);
const phraseOptions = computed(() => {
  const examples =
    draft.value.wakeLanguage === "ru"
      ? ["эй фоно", "привет компьютер"]
      : ["hey fono", "hello computer"];
  const current = draft.value.wakePhrase;
  if (current && !examples.includes(current)) examples.unshift(current);
  return examples.map((value) => ({ value, label: value }));
});
</script>
<template>
  <div class="form-stack">
    <template v-if="scope !== 'wake' && scope !== 'command-hotkey'">
      <SelectField
        v-if="LIVE_DICTATION_ENABLED"
        id="dictationMode"
        v-model="draft.dictationMode"
        :label="$t('ui.a6d9bf3c4d')"
        :disabled="active"
        :options="[
          {
            value: 'standard',
            get label() {
              return $t('ui.1db14eb858');
            },
          },
          {
            value: 'live',
            get label() {
              return $t('ui.797e3c64db');
            },
          },
        ]"
        :hint="$t('ui.5307514b9d')"
      />
    </template>
    <TriggerFields
      v-if="scope !== 'wake'"
      v-model="draft"
      :scope="
        scope === 'hotkey' || scope === 'command-hotkey' ? scope : undefined
      "
      :active="active"
    />
    <SelectionFields v-if="!scope" v-model="draft" :active="active" />
    <template
      v-if="wakeCompiled && scope !== 'hotkey' && scope !== 'command-hotkey'"
    >
      <p v-if="!WAKE_WORD_AVAILABLE" class="notice" role="status">
        {{ unavailableWakeMessage() }}. {{ alternativeWakeMessage() }}
      </p>
      <PreferenceToggle
        name="wakeEnabled"
        :label="$t('ui.3e8b54eb6f')"
        :description="$t('ui.4919634c1d')"
      />
      <SelectField
        id="wakeLanguage"
        v-model="draft.wakeLanguage"
        :label="$t('ui.b22393da4c')"
        :options="wakeLanguages"
        :disabled="!WAKE_WORD_AVAILABLE"
        :hint="$t('ui.7bf38d891c')"
      />
      <SelectField
        v-model="draft.wakePhrase"
        :label="$t('ui.d26172ff4e')"
        :options="phraseOptions"
        :disabled="!WAKE_WORD_AVAILABLE"
        :hint="$t('ui.edcd254e2a')"
      />
      <p
        v-if="workspace.state.wakePhrases?.length"
        class="muted wake-phrase-catalog"
      >
        {{ $t("ui.82d62de9da") }} {{ workspace.state.wakePhrases.join(" · ") }}
      </p>
      <WlField
        v-slot="field"
        id="wakePhrase"
        :label="$t('ui.78d427f63a')"
        :hint="
          draft.wakeLanguage === 'ru'
            ? $t('ui.d5d6ef5236')
            : $t('ui.0dc5a41fdc')
        "
      >
        <WlInput
          v-bind="field"
          v-model="draft.wakePhrase"
          :maxlength="80"
          :disabled="!WAKE_WORD_AVAILABLE"
        />
      </WlField>
      <SelectField
        v-if="draft.dictationMode === 'standard'"
        id="silenceMs"
        v-model="draft.silenceMs"
        :label="$t('ui.8a06c28498')"
        :disabled="!WAKE_WORD_AVAILABLE"
        :options="[
          {
            value: 800,
            get label() {
              return $t('ui.eadadca66e');
            },
          },
          {
            value: 1600,
            get label() {
              return $t('ui.294f67afea');
            },
          },
          {
            value: 2000,
            get label() {
              return $t('ui.d68d60f8f9');
            },
          },
          {
            value: draft.silenceMs,
            label: draft.silenceMs / 1000 + $t('ui.e88f4fb7cb'),
          },
          {
            value: 2500,
            get label() {
              return $t('ui.ce4a27a957');
            },
          },
          {
            value: 4000,
            get label() {
              return $t('ui.46ac6deeb0');
            },
          },
        ]"
      />
      <p v-else class="notice">{{ $t("ui.0dd5ffc00e") }}</p>
      <NativeWakeSetup
        v-if="workspace.wake"
        :phrase="draft.wakePhrase"
        :language="draft.wakeLanguage"
      />
      <details v-if="advanced" id="advanced" class="advanced">
        <summary>{{ $t("ui.eff08b3cea") }}</summary>
        <div class="form-stack">
          <p class="muted">{{ $t("ui.39db9049f8") }}</p>
          <label
            >{{ $t("ui.d7d3835d63") }}
            {{ Math.round(draft.wakeThreshold * 100) }}%
            <WlSlider
              v-model="draft.wakeThreshold"
              :min="0.1"
              :max="0.9"
              :step="0.05"
              :disabled="!WAKE_WORD_AVAILABLE"
              :aria-label="$t('ui.e59808a18f')"
            />
          </label>
          <label
            >{{ $t("ui.3baa924131") }}
            {{ Math.round(draft.speechThreshold * 100) }}%
            <WlSlider
              v-model="draft.speechThreshold"
              :min="workspace.native ? 0.001 : 0.1"
              :max="workspace.native ? 0.05 : 0.9"
              :step="workspace.native ? 0.001 : 0.05"
              :disabled="!WAKE_WORD_AVAILABLE"
              :aria-label="$t('ui.5e3399ce17')"
            />
          </label>
        </div>
      </details>
    </template>
  </div>
</template>
