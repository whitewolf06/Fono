<script setup lang="ts">
import AppIcon from "../../../shared/presentation/AppIcon.vue";
withDefaults(defineProps<{ hotkeyMode?: "hold" | "toggle" }>(), {
  hotkeyMode: "toggle",
});
</script>
<template>
  <p>
    <AppIcon name="check" :size="15" class="accept" /><span
      ><strong>Принять кнопкой</strong> — получить текст здесь, без вставки в
      другое окно.</span
    >
  </p>
  <p>
    <AppIcon name="copy" :size="15" class="copy" /><span
      ><strong>Копировать</strong> — скопировать готовый текст и закрыть
      индикатор.</span
    >
  </p>
  <p>
    <AppIcon name="x" :size="15" class="cancel" /><span
      ><strong>Крестик</strong> — отменить запись или закрыть готовый
      результат.</span
    >
  </p>
  <p>
    <AppIcon name="keyboard" :size="15" /><span
      ><strong>{{
        hotkeyMode === "hold"
          ? "Отпустить сочетание клавиш"
          : "Повторное сочетание клавиш"
      }}</strong>
      — завершить запись и вставить текст в исходное поле и закрыть
      индикатор.</span
    >
  </p>
  <p>
    <AppIcon name="sparkle" :size="15" class="processing" /><span
      ><strong>Искры</strong> — постобработка. Переключатель включает её,
      шестерёнка выбирает стиль.</span
    >
  </p>
  <p>
    <AppIcon name="translate" :size="15" class="translation" /><span
      ><strong>Перевод</strong> — переключатель добавляет перевод после
      обработки, шестерёнка выбирает язык.</span
    >
  </p>
  <small
    >Стиль и перевод можно менять во время речи. Выбор запоминается для
    следующей записи.</small
  >
</template>
