<script setup lang="ts">
import type { Health } from "../domain/contracts";
defineProps<{ health: Health; label?: string }>();
const labels: Record<Health, string> = {
  ready: "Готово",
  off: "Выключено",
  missing: "Нужна настройка",
  loading: "Загрузка",
  error: "Ошибка",
};
</script>
<template>
  <span class="status" :data-health="health"
    ><span class="status-dot" aria-hidden="true" />{{
      label || labels[health]
    }}</span
  >
</template>
