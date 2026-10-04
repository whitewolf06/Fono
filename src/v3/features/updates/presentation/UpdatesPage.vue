<script setup lang="ts">
import { RouterLink } from "vue-router";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import UpdatesPanel from "./UpdatesPanel.vue";
</script>
<template>
  <div class="page updates-page">
    <PageHeading
      title="Обновления"
      description="Новые версии Fono и управление установкой."
    >
      <RouterLink class="text-link" to="/releases"
        ><AppIcon name="note" :size="16" />История версий</RouterLink
      >
    </PageHeading>
    <div class="updates-page-content">
      <UpdatesPanel :unsaved="false" />
      <div class="updates-page-note">
        <AppIcon name="info" :size="16" />
        <p>
          Перед установкой Fono проверяет подпись обновления. Установка закроет
          приложение: завершите диктовку и сохраните текущий текст.
        </p>
      </div>
    </div>
  </div>
</template>
<style scoped>
.updates-page-content {
  max-width: var(--fono-live-notice-width);
  padding: var(--fono-space-5);
  border: 1px solid var(--fono-border);
  border-radius: var(--fono-radius);
  background: var(--fono-surface);
}
.updates-page-content :deep(.update-settings) {
  border-top: 0;
  padding-top: 0;
}
.updates-page-note {
  display: flex;
  align-items: flex-start;
  gap: var(--fono-space-2);
  padding-top: var(--fono-space-4);
  margin-top: var(--fono-space-5);
  border-top: 1px solid var(--fono-border-soft);
  color: var(--fono-muted);
}
.updates-page-note svg {
  margin-top: var(--fono-space-1);
}
.updates-page-note p {
  color: var(--fono-muted);
  font-size: var(--fono-type-sm);
}
</style>
