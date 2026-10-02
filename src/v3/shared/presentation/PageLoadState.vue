<script setup lang="ts">
import { WlButton, WlSkeleton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../application/workspace";
import AppIcon from "./AppIcon.vue";
const workspace = useWorkspace();
</script>
<template>
  <div class="page">
    <div
      v-if="workspace.state.scenario === 'loading'"
      class="loading-page"
      role="status"
      aria-live="polite"
    >
      <span class="eyebrow">Загружаем данные</span
      ><WlSkeleton width="240px" height="30px" /><WlSkeleton
        width="70%"
        height="16px"
      /><WlSkeleton width="100%" height="150px" /><WlSkeleton
        width="100%"
        height="100px"
      />
      <p class="muted">Подготавливаем раздел…</p>
      <RouterLink v-if="!workspace.native" class="text-link" to="/scenarios"
        >К сценариям проверки</RouterLink
      >
    </div>
    <div v-else class="empty-state" role="alert">
      <AppIcon name="warn" :size="30" />
      <h2>Не удалось загрузить данные</h2>
      <p v-if="workspace.native">{{ workspace.state.error }}</p>
      <p v-else>
        Демонстрационная ошибка соединения. Настройки и текущий текст сохранены
        в памяти сессии.
      </p>
      <WlButton @click="workspace.scenario('normal')"
        >Попробовать снова</WlButton
      ><RouterLink v-if="!workspace.native" class="text-link" to="/scenarios"
        >К сценариям проверки</RouterLink
      >
    </div>
  </div>
</template>
