<script setup lang="ts">
import { WlButton, WlEmpty } from "@whitelife-core/ui-kit";
import { formatHistoryDate, type VoiceEntry } from "../domain/voice";

defineProps<{ history: VoiceEntry[] }>();
const emit = defineEmits<{ refresh: []; copy: [text: string] }>();
</script>

<template>
  <section class="v3-panel v3-history" aria-labelledby="v3-history-title">
    <div class="v3-section-heading">
      <div>
        <span class="v3-heading-icon"><i class="pi pi-history"></i></span>
        <h2 id="v3-history-title">Последние транскрибации</h2>
      </div>
      <WlButton variant="ghost" size="sm" @click="emit('refresh')"
        ><i class="pi pi-refresh"></i> Обновить</WlButton
      >
    </div>
    <WlEmpty
      v-if="!history.length"
      title="Пока нет записей"
      description="Начните диктовку, и результат появится здесь."
    />
    <div v-else class="v3-history-table">
      <div class="v3-history-table-head">
        <span>Текст</span><span>Время</span><span>Действие</span>
      </div>
      <div
        v-for="entry in history.slice(0, 5)"
        :key="entry.id"
        class="v3-history-row"
      >
        <span class="v3-history-text"
          ><i class="pi pi-file-edit"></i
          ><span :title="entry.text">{{ entry.text }}</span></span
        >
        <time>{{ formatHistoryDate(entry.createdAt) }}</time>
        <WlButton
          variant="ghost"
          size="sm"
          :aria-label="
            'Скопировать запись от ' + formatHistoryDate(entry.createdAt)
          "
          @click="emit('copy', entry.text)"
          ><i class="pi pi-copy"></i
        ></WlButton>
      </div>
    </div>
  </section>
</template>
