<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { ref } from "vue";
import { useFeedback } from "../../../shared/application/feedback";
import { WlButton, WlInput, WlField } from "@whitelife-core/ui-kit";
import type { LaunchApp } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
import { supportedCommands } from "../domain/catalog";
import AppEditor from "./AppEditor.vue";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
const workspace = useWorkspace();
const { run } = useFeedback();
const ui = useInteraction();
const phrase = ref("");
const result = ref("");
const editing = ref<LaunchApp | null>(null);
async function remove(app: LaunchApp) {
  if (
    await ui.confirm({
      get title() {
        return uiMessage("ui.35467365fd");
      },
      text:
        uiMessage("ui.2b8e42c7ce") + app.phrase + uiMessage("ui.88d3d7d3e1"),
      get accept() {
        return uiMessage("ui.be99b13612");
      },
      danger: true,
    })
  )
    await run(() => workspace.commands.removeApp(app.id));
}
</script>
<template>
  <div class="page">
    <PageHeading :title="$t('ui.6c0adb50d0')" :description="$t('ui.c082fbafd9')"
      ><WlButton size="sm" @click="ui.openQuick('command-hotkey')"
        >{{ workspace.state.preferences.commandHotkey
        }}<template #icon><AppIcon name="edit" /></template></WlButton
    ></PageHeading>
    <section class="surface-panel">
      <h2>{{ $t("ui.2370696018") }}</h2>
      <p>{{ $t("ui.46226a7f52") }}</p>
      <WlField v-slot="field" :label="$t('ui.5fe3b7c928')"
        ><div class="inline-input">
          <WlInput
            v-bind="field"
            v-model="phrase"
            :placeholder="$t('ui.448a6cb4c0')"
            @keydown.enter.prevent="result = workspace.commands.test(phrase)"
          /><WlButton
            :disabled="!phrase.trim()"
            @click="result = workspace.commands.test(phrase)"
            >{{ $t("ui.e4424a6df6") }}</WlButton
          >
        </div></WlField
      >
      <p v-if="result" class="notice" role="status">{{ result }}</p>
    </section>
    <div class="command-groups">
      <section
        v-for="group in [
          $t('ui.2f2bfeb36e'),
          $t('ui.6a40ca9613'),
          $t('ui.1e1bd9e6df'),
        ]"
        :key="group"
        class="surface-panel"
      >
        <h2>{{ group }}</h2>
        <div
          v-for="command in supportedCommands.filter((c) => c.group === group)"
          :key="command.phrase"
          class="command-item"
        >
          <button
            class="text-link"
            @click="
              phrase = command.phrase;
              result = workspace.commands.test(command.phrase);
            "
          >
            «{{ command.phrase }}»</button
          ><small>{{ command.description }}</small>
        </div>
      </section>
    </div>
    <section class="surface-panel">
      <div class="section-header">
        <div>
          <h2>{{ $t("ui.55eecd9fc6") }}</h2>
          <p>{{ $t("ui.6ed5c141eb") }}</p>
        </div>
        <WlButton
          size="sm"
          @click="editing = { id: '', name: '', phrase: '', path: '' }"
          ><template #icon><AppIcon name="plus" /></template
          >{{ $t("ui.71038c53bb") }}</WlButton
        >
      </div>
      <div
        v-for="app in workspace.state.applications"
        :key="app.id"
        class="app-row"
      >
        <AppIcon name="grid" />
        <div>
          <strong>{{ app.name }}</strong>
          <p>«{{ app.phrase }}»</p>
          <small class="muted">{{ app.path }}</small>
        </div>
        <WlButton
          size="sm"
          variant="ghost"
          :aria-label="$t('ui.669958f765') + app.name"
          @click="editing = { ...app }"
          ><template #icon><AppIcon name="edit" /></template></WlButton
        ><WlButton
          size="sm"
          variant="danger-quiet"
          :aria-label="$t('ui.1c6789d896') + app.name"
          @click="remove(app)"
          ><template #icon><AppIcon name="trash" /></template
        ></WlButton>
      </div>
      <p v-if="!workspace.state.applications.length" class="muted">
        {{ $t("ui.fc20f42119") }}
      </p>
    </section>
    <AppEditor v-if="editing" :application="editing" @close="editing = null" />
  </div>
</template>
