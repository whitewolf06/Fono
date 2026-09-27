<script setup lang="ts">
import { formatHistoryDate, type VoiceEntry } from "../domain/voice";

defineProps<{ history: VoiceEntry[]; hotkey: string; version: string }>();
</script>

<template>
  <aside class="v3-rail" aria-label="Активность и подсказки">
    <section class="v3-rail-section" aria-labelledby="v3-activity-title">
      <div class="v3-rail-heading">
        <h2 id="v3-activity-title">Недавняя активность</h2>
        <a href="#v3-history-title" aria-label="К истории"
          ><i class="pi pi-arrow-right"></i
        ></a>
      </div>
      <p v-if="!history.length" class="v3-rail-empty">Записей пока нет.</p>
      <a
        v-for="entry in history.slice(0, 5)"
        :key="entry.id"
        class="v3-activity-item"
        href="#v3-history-title"
      >
        <span class="v3-activity-icon"><i class="pi pi-file"></i></span>
        <span class="v3-activity-copy"
          ><strong>{{ entry.text }}</strong
          ><time>{{ formatHistoryDate(entry.createdAt) }}</time></span
        >
      </a>
    </section>
    <section class="v3-panel v3-tips" aria-labelledby="v3-tips-title">
      <h2 id="v3-tips-title">
        <i class="pi pi-lightbulb"></i> Полезные советы
      </h2>
      <p>
        <i class="pi pi-bolt"></i
        ><span
          >Используйте <strong>{{ hotkey }}</strong> для быстрого старта.</span
        >
      </p>
      <p>
        <i class="pi pi-file-edit"></i
        ><span>Текст вставится в активное поле.</span>
      </p>
      <p>
        <i class="pi pi-microphone"></i
        ><span>Голосовая фраза поможет начать без клавиатуры.</span>
      </p>
      <div class="v3-rail-footer">Говорить проще, чем печатать.</div>
    </section>
    <div class="v3-version">Fono {{ version }} · интерфейс V3</div>
  </aside>
</template>
