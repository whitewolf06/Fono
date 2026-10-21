<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";
import { translateAuthored } from "../../../shared/domain/messages";

import { formatVersionDate, type VersionEntry } from "../domain/versionHistory";
defineProps<{ entry: VersionEntry; current?: boolean }>();
</script>
<template>
  <article
    class="version-note"
    :class="{ 'is-current': current }"
    :aria-label="`Fono ${entry.version}: ${translateAuthored(entry.title)}`"
  >
    <header>
      <div class="version-note-meta">
        <strong>Fono {{ entry.version }}</strong
        ><span v-if="current" class="version-current-label">{{
          $t("ui.fb305b470a")
        }}</span
        ><time :datetime="entry.date">{{ formatVersionDate(entry.date) }}</time>
      </div>
      <h3>{{ translateAuthored(entry.title) }}</h3>
    </header>
    <ul>
      <li v-for="change in entry.changes" :key="change">
        {{ translateAuthored(change) }}
      </li>
    </ul>
  </article>
</template>
