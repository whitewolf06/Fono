<script setup lang="ts">
import type { VoiceEntry } from "../domain/voice";

defineProps<{ entry: VoiceEntry | null }>();
const emit = defineEmits<{ "open-history": [] }>();
</script>

<template>
  <section class="v3-text-preview" aria-labelledby="v3-text-preview-title">
    <div class="v3-text-preview-heading">
      <h2 id="v3-text-preview-title">Последний текст</h2>
      <button type="button" @click="emit('open-history')">
        <i class="pi pi-history" aria-hidden="true"></i>
        История
        <i class="pi pi-arrow-right" aria-hidden="true"></i>
      </button>
    </div>
    <textarea
      aria-label="Последний распознанный текст"
      :value="entry?.text ?? ''"
      placeholder="После диктовки здесь появится текст."
      readonly
      rows="2"
    ></textarea>
  </section>
</template>
