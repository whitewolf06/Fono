<script setup lang="ts">
defineProps<{ title: string; description?: string; eyebrow?: string }>();
</script>
<template>
  <header class="page-heading">
    <div>
      <small v-if="eyebrow" class="eyebrow">{{ eyebrow }}</small>
      <h1>{{ title }}</h1>
      <p v-if="description">{{ description }}</p>
    </div>
    <div class="actions"><slot /></div>
  </header>
</template>
