<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import { useInteraction } from "../../../shared/application/interaction";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import { computed } from "vue";
import { engineNames } from "../../../shared/domain/speechModels";
import { measurementContextMessage } from "../../../shared/domain/sttMeasurements";
const workspace = useWorkspace();
const { run } = useFeedback();
const ui = useInteraction();
const groups = computed(() =>
  (Object.keys(engineNames) as (keyof typeof engineNames)[])
    .map((engine) => ({
      engine,
      title: engineNames[engine],
      models: workspace.state.models.filter(
        (m) => (m.engine || "whisper") === engine,
      ),
    }))
    .filter((group) => group.models.length),
);
const readiness = computed(() => {
  const status = workspace.state.sttReadiness;
  if (!workspace.native || !status) return "";
  if (status.state === "loading") return uiMessage("ui.4f6bc505e8");
  if (status.state === "ready")
    return (
      uiMessage("ui.61de22bfdc") + (status.device || uiMessage("ui.329bcc74f8"))
    );
  if (status.state === "failed")
    return uiMessage("ui.f1a7bf229f") + status.message;
  return uiMessage("ui.27d278129d");
});
async function remove(id: string) {
  if (
    await ui.confirm({
      get title() {
        return uiMessage("ui.6463a43017");
      },
      text:
        uiMessage("ui.f17acf4460") +
        (workspace.native
          ? uiMessage("ui.e97248e236")
          : uiMessage("ui.ea89bde9ad")),
      get accept() {
        return uiMessage("ui.46cb5952c6");
      },
      danger: true,
    })
  )
    await run(() => workspace.settings.removeModel(id));
}
</script>
<template>
  <section class="model-manager">
    <h3>{{ $t("ui.68db4cb08c") }}</h3>
    <p class="muted">
      {{ workspace.native ? $t("ui.8451e6adab") : $t("ui.43e32779b3") }}
    </p>
    <p v-if="readiness" class="muted" role="status">{{ readiness }}</p>
    <p class="muted">
      {{ measurementContextMessage() }}
    </p>
    <p class="muted">{{ $t("ui.55c23a4926") }}</p>
    <section
      v-for="group in groups"
      :key="group.engine"
      :aria-label="group.title"
    >
      <h4>{{ group.title }}</h4>
      <div v-for="model in group.models" :key="model.id" class="model-row">
        <AppIcon name="inbox" />
        <div>
          <strong>{{ model.name }}</strong>
          <p>{{ model.description }} · {{ model.size }}</p>
          <p v-if="model.benchmark" class="muted">{{ model.benchmark }}</p>
          <p v-if="model.languages">
            {{ $t("ui.b449b5b0c5") }}
            {{ model.languages.join(", ").toUpperCase()
            }}{{ model.multilingual ? $t("ui.46f84d60e0") : "" }}
            {{ $t("ui.3fd523791a") }}
            {{ model.punctuation ? $t("ui.747f78e5a0") : $t("ui.dc9337ff5b") }}
          </p>
          <p v-if="model.license">
            <a :href="model.licenseUrl" target="_blank" rel="noreferrer">{{
              model.license
            }}</a>
            · {{ model.engine === "whisper" ? $t("ui.eaa780233f") : "CPU"
            }}<span v-if="workspace.state.preferences.model === model.id">
              {{ $t("ui.e1f5841acc") }}</span
            >
          </p>
          <p
            v-if="
              model.downloadPhase === 'verifying' &&
              model.status === 'downloading'
            "
            role="status"
          >
            {{ $t("ui.44fb23157e") }}
          </p>
          <p v-if="model.downloadError" class="error-text" role="alert">
            {{ model.downloadError }}
          </p>
          <progress
            v-if="model.status === 'downloading'"
            :value="model.progress"
            max="100"
            :aria-label="$t('ui.20833e01ff') + model.name"
          />
        </div>
        <WlButton
          v-if="model.status === 'available'"
          size="sm"
          @click="run(() => workspace.settings.downloadModel(model.id))"
          ><template #icon><AppIcon name="download" /></template
          >{{ $t("ui.9053d64435") }}</WlButton
        >
        <WlButton
          v-else-if="model.status === 'downloading'"
          size="sm"
          variant="ghost"
          @click="run(() => workspace.settings.removeModel(model.id))"
          >{{ $t("ui.7c47f7292b") }} {{ model.progress }}%</WlButton
        >
        <WlButton
          v-else-if="!model.custom"
          size="sm"
          variant="ghost"
          :disabled="workspace.state.preferences.model === model.id"
          :aria-label="$t('ui.1c6789d896') + model.name"
          @click="remove(model.id)"
          ><template #icon><AppIcon name="trash" /></template
          >{{ $t("ui.be99b13612") }}</WlButton
        >
      </div>
    </section>
  </section>
</template>
