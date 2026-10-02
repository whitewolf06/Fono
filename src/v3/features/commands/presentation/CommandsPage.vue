<script setup lang="ts">
import { ref } from "vue";
import { WlButton, WlInput, WlField } from "@whitelife-core/ui-kit";
import type { LaunchApp } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
import { supportedCommands } from "../domain/catalog";
import AppEditor from "./AppEditor.vue";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
const workspace = useWorkspace();
const ui = useInteraction();
const phrase = ref("");
const result = ref("");
const editing = ref<LaunchApp | null>(null);
async function remove(app: LaunchApp) {
  if (
    await ui.confirm({
      title: "Удалить голосовую команду?",
      text: "Фраза «" + app.phrase + "» больше не будет открывать приложение.",
      accept: "Удалить",
      danger: true,
    })
  )
    workspace.commands.removeApp(app.id);
}
</script>
<template>
  <div class="page">
    <PageHeading
      title="Голосовые команды"
      description="Управляйте привычными действиями голосом."
      ><WlButton size="sm" @click="ui.openQuick('command-hotkey')"
        >{{ workspace.state.preferences.commandHotkey
        }}<template #icon><AppIcon name="edit" /></template></WlButton
    ></PageHeading>
    <section class="surface-panel">
      <h2>Проверить фразу</h2>
      <p>Введите команду — Fono покажет, что произойдёт.</p>
      <WlField v-slot="field" label="Фраза"
        ><div class="inline-input">
          <WlInput
            v-bind="field"
            v-model="phrase"
            placeholder="Например, открой блокнот"
            @keydown.enter.prevent="result = workspace.commands.test(phrase)"
          /><WlButton
            :disabled="!phrase.trim()"
            @click="result = workspace.commands.test(phrase)"
            >Проверить</WlButton
          >
        </div></WlField
      >
      <p v-if="result" class="notice" role="status">{{ result }}</p>
    </section>
    <div class="command-groups">
      <section
        v-for="group in ['Медиа', 'Звук', 'Окна']"
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
          <h2>Ваши приложения</h2>
          <p>Название, которое удобно произносить.</p>
        </div>
        <WlButton
          size="sm"
          @click="editing = { id: '', name: '', phrase: '', path: '' }"
          ><template #icon><AppIcon name="plus" /></template>Добавить</WlButton
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
          :aria-label="'Изменить ' + app.name"
          @click="editing = { ...app }"
          ><template #icon><AppIcon name="edit" /></template></WlButton
        ><WlButton
          size="sm"
          variant="danger-quiet"
          :aria-label="'Удалить ' + app.name"
          @click="remove(app)"
          ><template #icon><AppIcon name="trash" /></template
        ></WlButton>
      </div>
      <p v-if="!workspace.state.applications.length" class="muted">
        Приложений пока нет. Добавьте первое.
      </p>
    </section>
    <AppEditor v-if="editing" :application="editing" @close="editing = null" />
  </div>
</template>
