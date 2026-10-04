<script setup lang="ts">
import { formatVersionDate, type VersionEntry } from "../domain/versionHistory";
defineProps<{ entry: VersionEntry; current?: boolean }>();
</script>
<template>
  <article
    class="version-note"
    :class="{ 'is-current': current }"
    :aria-label="`Fono ${entry.version}: ${entry.title}`"
  >
    <header>
      <div class="version-note-meta">
        <strong>Fono {{ entry.version }}</strong
        ><span v-if="current" class="version-current-label">Текущая</span
        ><time :datetime="entry.date">{{ formatVersionDate(entry.date) }}</time>
      </div>
      <h3>{{ entry.title }}</h3>
    </header>
    <ul>
      <li v-for="change in entry.changes" :key="change">{{ change }}</li>
    </ul>
  </article>
</template>
