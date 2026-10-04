<script setup lang="ts">
import type { Preferences } from "../../../shared/domain/contracts";
defineProps<{ preferences: Preferences; pending?: boolean }>();
</script>
<template>
  <aside
    class="overlay-help"
    aria-label="Пояснения к индикатору"
    data-overlay-interactive
  >
    <p class="dictation-action--insert">
      {{
        pending
          ? "Исходный — принять текст без ИИ."
          : "Завершить — остановить запись и распознать речь."
      }}
    </p>
    <p class="dictation-action--process">
      Стиль — очистка, структура, задача или деловой тон.
    </p>
    <p class="dictation-action--translate">
      Перевод — обработать и перевести на выбранный язык.
    </p>
    <p class="dictation-action--cancel">Отмена — закончить без вставки.</p>
    <small>{{
      preferences.processingTrigger === "automatic"
        ? "Авто: выбранная обработка запускается после записи."
        : "По кнопке: после распознавания выберите «Обработать» или «Исходный»."
    }}</small>
  </aside>
</template>
