<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { WlButton } from "@whitelife-core/ui-kit";
import type { Scenario } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import { LIVE_DICTATION_ENABLED } from "../../../shared/domain/dictationMode";
import {
  WAKE_WORD_AVAILABLE,
  unavailableWakeMessage,
} from "../../../shared/domain/wakeAvailability";
const workspace = useWorkspace();
const scenarios: { id: Scenario; title: string; detail: string }[] = [
  {
    id: "live-paused",
    get title() {
      return uiMessage("ui.b3c0602884");
    },
    get detail() {
      return uiMessage("ui.a7faef2102");
    },
  },
  {
    id: "live-backlog",
    get title() {
      return uiMessage("ui.f2b6392c4a");
    },
    get detail() {
      return uiMessage("ui.c67bcab87b");
    },
  },
  {
    id: "live-insertion-error",
    get title() {
      return uiMessage("ui.d490f3c570");
    },
    get detail() {
      return uiMessage("ui.2df8c7ce13");
    },
  },
  {
    id: "loading",
    get title() {
      return uiMessage("ui.8a4db3e86d");
    },
    get detail() {
      return uiMessage("ui.649d1c824c");
    },
  },
  {
    id: "load-error",
    get title() {
      return uiMessage("ui.bd54852c08");
    },
    get detail() {
      return uiMessage("ui.e697f97df8");
    },
  },
  {
    id: "normal",
    get title() {
      return uiMessage("ui.e6ae02f6d3");
    },
    get detail() {
      return uiMessage("ui.af79605242");
    },
  },
  {
    id: "empty",
    get title() {
      return uiMessage("ui.1152fbb8a1");
    },
    get detail() {
      return uiMessage("ui.ef6095b4f4");
    },
  },
  {
    id: "no-microphone",
    get title() {
      return uiMessage("ui.86a86ac611");
    },
    get detail() {
      return uiMessage("ui.6c64c14630");
    },
  },
  {
    id: "no-model",
    get title() {
      return uiMessage("ui.696943d49f");
    },
    get detail() {
      return uiMessage("ui.a7b6c55d0c");
    },
  },
  {
    id: "download",
    get title() {
      return uiMessage("ui.99e88b8410");
    },
    get detail() {
      return uiMessage("ui.b2e0c8472b");
    },
  },
  {
    id: "ai-error",
    get title() {
      return uiMessage("ui.230fdc7b1a");
    },
    get detail() {
      return uiMessage("ui.8c75672117");
    },
  },
  {
    id: "save-error",
    get title() {
      return uiMessage("ui.354f0fb38a");
    },
    get detail() {
      return uiMessage("ui.41098595b6");
    },
  },
  {
    id: "service-off",
    get title() {
      return uiMessage("ui.ed913eabb5");
    },
    get detail() {
      return uiMessage("ui.cb673ef19f");
    },
  },
  {
    id: "queue",
    get title() {
      return uiMessage("ui.bfa4ea4160");
    },
    get detail() {
      return uiMessage("ui.46be1219c5");
    },
  },
  {
    id: "long-content",
    get title() {
      return uiMessage("ui.1536aed490");
    },
    get detail() {
      return uiMessage("ui.d3714fd90a");
    },
  },
];
</script>
<template>
  <div class="page">
    <PageHeading :title="$t('ui.228609c7a8')" :description="$t('ui.b62d31f826')"
      ><RouterLink class="text-link" to="/">{{
        $t("ui.feee5f4aa5")
      }}</RouterLink></PageHeading
    >
    <p class="notice">{{ $t("ui.0d34761439") }}</p>
    <p v-if="!WAKE_WORD_AVAILABLE" class="notice">
      {{ $t("ui.b6d7b2cfa2") }} {{ unavailableWakeMessage()
      }}{{ $t("ui.49a5d2328e") }}
    </p>
    <div class="scenario-grid">
      <button
        v-for="scenario in scenarios.filter(
          (item) => LIVE_DICTATION_ENABLED || !item.id.startsWith('live-'),
        )"
        :key="scenario.id"
        class="scenario-card"
        :class="{ selected: workspace.state.scenario === scenario.id }"
        :aria-pressed="workspace.state.scenario === scenario.id"
        @click="workspace.scenario(scenario.id)"
      >
        <div class="section-header">
          <h3>{{ scenario.title }}</h3>
          <AppIcon
            :name="workspace.state.scenario === scenario.id ? 'check' : 'grid'"
          />
        </div>
        <p>{{ scenario.detail }}</p>
      </button>
    </div>
    <div class="surface-panel section-header">
      <div>
        <h2>{{ $t("ui.d7cea752bf") }}</h2>
        <p>{{ $t("ui.fba1204200") }}</p>
      </div>
      <div class="actions">
        <RouterLink class="text-link" to="/onboarding">{{
          $t("ui.9980024609")
        }}</RouterLink
        ><RouterLink class="text-link" to="/overlay">{{
          $t("ui.284bb35a90")
        }}</RouterLink
        ><WlButton size="sm" @click="workspace.scenario('normal')">{{
          $t("ui.8f88dbc5cd")
        }}</WlButton>
      </div>
    </div>
  </div>
</template>
