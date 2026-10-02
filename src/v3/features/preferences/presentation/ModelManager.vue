<script setup lang="ts">
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import { useInteraction } from "../../../shared/application/interaction";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
const workspace = useWorkspace();
const { run } = useFeedback();
const ui = useInteraction();
async function remove(id: string) {
  if (
    await ui.confirm({
      title: "Удалить модель?",
      text: "Чтобы снова использовать модель, потребуется загрузить её. В демо файлы не меняются.",
      accept: "Удалить модель",
      danger: true,
    })
  )
    workspace.settings.removeModel(id);
}
</script>
<template>
  <section class="model-manager">
    <h3>Модели на компьютере</h3>
    <p class="muted">Загрузка и объём показаны для демонстрации.</p>
    <div
      v-for="model in workspace.state.models"
      :key="model.id"
      class="model-row"
    >
      <AppIcon name="inbox" />
      <div>
        <strong>{{ model.name }}</strong>
        <p>{{ model.description }} · {{ model.size }}</p>
        <progress
          v-if="model.status === 'downloading'"
          :value="model.progress"
          max="100"
          :aria-label="'Загрузка ' + model.name"
        />
      </div>
      <WlButton
        v-if="model.status === 'available'"
        size="sm"
        @click="run(() => workspace.settings.downloadModel(model.id))"
        ><template #icon><AppIcon name="download" /></template
        >Загрузить</WlButton
      >
      <WlButton
        v-else-if="model.status === 'downloading'"
        size="sm"
        variant="ghost"
        @click="workspace.settings.removeModel(model.id)"
        >Отменить {{ model.progress }}%</WlButton
      >
      <WlButton
        v-else
        size="sm"
        variant="ghost"
        :aria-label="'Удалить ' + model.name"
        @click="remove(model.id)"
        ><template #icon><AppIcon name="trash" /></template>Удалить</WlButton
      >
    </div>
  </section>
</template>
