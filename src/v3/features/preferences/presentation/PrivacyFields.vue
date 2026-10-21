<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { useFeedback } from "../../../shared/application/feedback";
import { WlButton } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import PreferenceToggle from "./PreferenceToggle.vue";
import SelectField from "../../../shared/presentation/SelectField.vue";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
const draft = defineModel<Preferences>({ required: true });
const workspace = useWorkspace();
const { run } = useFeedback();
const ui = useInteraction();
async function clear(kind: "history" | "trainer") {
  if (
    await ui.confirm({
      title:
        kind === "history"
          ? uiMessage("ui.da7d13ad43")
          : uiMessage("ui.7de07d3c5a"),
      text:
        kind === "history"
          ? uiMessage("ui.208c6cb3b4")
          : uiMessage("ui.12c0f354cd"),
      get accept() {
        return uiMessage("ui.a64ec93960");
      },
      danger: true,
    })
  ) {
    await run(() =>
      kind === "history"
        ? workspace.history.clear()
        : workspace.trainer.clear(),
    );
  }
}
</script>
<template>
  <div class="form-stack">
    <PreferenceToggle
      name="historyEnabled"
      :label="$t('ui.f0211033d5')"
      :description="$t('ui.d81998629a')"
    /><SelectField
      v-if="workspace.native"
      id="historyRetentionDays"
      v-model="draft.historyRetentionDays"
      :label="$t('ui.ad264afe23')"
      :options="[
        {
          value: 7,
          get label() {
            return $t('ui.7867a2095f');
          },
        },
        {
          value: 30,
          get label() {
            return $t('ui.254d55672b');
          },
        },
        {
          value: 90,
          get label() {
            return $t('ui.add2e2fe30');
          },
        },
        {
          value: 365,
          get label() {
            return $t('ui.3f9ab002f8');
          },
        },
        {
          value: 3650,
          get label() {
            return $t('ui.305189aafa');
          },
        },
      ]"
    /><PreferenceToggle
      name="trainerEnabled"
      :label="$t('ui.387054796e')"
      :description="$t('ui.e5139de357')"
    /><PreferenceToggle
      name="analyticsConsent"
      :label="$t('ui.b0f0680621')"
      :description="$t('ui.66ed3f363c')"
    /><SelectField
      id="retentionDays"
      v-model="draft.retentionDays"
      :label="workspace.native ? $t('ui.3cff493e39') : $t('ui.9334533cfa')"
      :options="[
        {
          value: 7,
          get label() {
            return $t('ui.7867a2095f');
          },
        },
        {
          value: 30,
          get label() {
            return $t('ui.254d55672b');
          },
        },
        {
          value: 90,
          get label() {
            return $t('ui.add2e2fe30');
          },
        },
        ...(workspace.native
          ? [
              {
                value: 365,
                get label() {
                  return $t('ui.3f9ab002f8');
                },
              },
            ]
          : [
              {
                value: 0,
                get label() {
                  return $t('ui.dc70e20f36');
                },
              },
            ]),
      ]"
    />
    <div v-if="workspace.native" class="notice">{{ $t("ui.fc1db948b8") }}</div>
    <div v-else class="notice">{{ $t("ui.9150c873d8") }}</div>
    <section class="danger-zone">
      <h3>{{ $t("ui.4a5474448a") }}</h3>
      <p>
        {{ workspace.state.history.length }} {{ $t("ui.4b6b16cfa7") }}
        {{ workspace.state.history.filter((e) => e.original).length }}
        {{ $t("ui.1a6305960f") }}
      </p>
      <div class="actions">
        <WlButton
          size="sm"
          variant="danger-quiet"
          :disabled="!workspace.state.history.length"
          @click="clear('history')"
          >{{ $t("ui.af40d9be9a") }}</WlButton
        ><WlButton
          size="sm"
          variant="danger-quiet"
          :disabled="!workspace.state.history.some((e) => e.original)"
          @click="clear('trainer')"
          >{{ $t("ui.c0d8dbac76") }}</WlButton
        >
      </div>
    </section>
  </div>
</template>
