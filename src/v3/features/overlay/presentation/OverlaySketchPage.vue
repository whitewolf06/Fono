<script setup lang="ts">
import { ref } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import { useOverlaySketch } from "../application/useOverlaySketch";
import { sketchTiming } from "../infrastructure/sketchTiming";
import OverlaySketch from "./OverlaySketch.vue";
const workspace = useWorkspace();
const compact = ref(false);
const fieldText = ref("");
const sketch = useOverlaySketch({
  ...sketchTiming,
  copy: (text) => workspace.copy(text),
  onInsert: (text) => {
    fieldText.value = text;
  },
});
const { state } = sketch;
function restart() {
  fieldText.value = "";
  sketch.start();
}
</script>
<template>
  <div class="page overlay-sketch-page">
    <PageHeading
      title="Новый оверлей"
      description="Интерактивный набросок. Три секции и только иконки действий."
    >
      <RouterLink class="text-link" to="/overlay"
        >Предыдущий вариант</RouterLink
      >
    </PageHeading>
    <div class="overlay-sketch-scenarios">
      <WlButton size="sm" @click="restart"
        ><template #icon><AppIcon name="microphone" :size="15" /></template
        >Новая запись</WlButton
      >
      <WlButton
        size="sm"
        :disabled="state.phase !== 'recording'"
        @click="sketch.finish('hotkey')"
        ><template #icon><AppIcon name="keyboard" :size="15" /></template
        >Завершить сочетанием · демо</WlButton
      >
      <WlButton size="sm" @click="compact = !compact">{{
        compact ? "Обычный размер" : "Компактный размер"
      }}</WlButton>
    </div>
    <div class="overlay-sketch-stage">
      <span class="overlay-sketch-stage-label">{{
        compact ? "Компактный индикатор" : "Индикатор записи"
      }}</span>
      <OverlaySketch
        v-if="state.phase !== 'closed'"
        :state="state"
        :compact="compact"
        @accept="sketch.finish('button')"
        @cancel="sketch.cancel"
        @close="sketch.close"
        @copy="sketch.copy"
        @choose="sketch.choose"
      />
      <div v-else class="overlay-sketch-closed" role="status">
        <AppIcon name="check" :size="24" /><strong>Индикатор закрыт</strong
        ><span>{{
          state.insertedText
            ? "Результат вставлен в демонстрационное поле."
            : "Нажмите «Новая запись», чтобы показать его снова."
        }}</span>
      </div>
      <div class="overlay-sketch-field">
        <span><AppIcon name="message" :size="14" />Внешнее поле · пример</span>
        <p v-if="fieldText" class="filled" role="status">{{ fieldText }}</p>
        <p v-else>
          Здесь появится текст только при завершении сочетанием клавиш. Нажатие
          галочки оставляет результат в оверлее.
        </p>
      </div>
    </div>
    <div class="overlay-sketch-notes">
      <p>
        <AppIcon name="check" :size="16" /><span
          ><strong>Галочка → текст → копирование</strong><br />Принятие кнопкой
          не вставляет текст. Копирование закрывает индикатор; крестик закрывает
          без копирования.</span
        >
      </p>
      <p>
        <AppIcon name="sparkle" :size="16" /><span
          ><strong>Переключатели слева от действий</strong><br />Постобработка и
          перевод меняются во время записи. Шестерёнки открывают выбор стиля и
          языка.</span
        >
      </p>
    </div>
  </div>
</template>
