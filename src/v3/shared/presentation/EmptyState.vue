<script setup lang="ts">
import AppIcon from "./AppIcon.vue";
defineProps<{ title: string; text: string }>();
</script>
<template>
  <div class="empty-state">
    <AppIcon name="inbox" :size="30" />
    <h3>{{ title }}</h3>
    <p>{{ text }}</p>
    <slot />
  </div>
</template>
