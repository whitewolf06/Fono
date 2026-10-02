<script setup lang="ts">
import { ref } from "vue";
import { WlField, WlInput, WlButton, WlSlider } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import PreferenceToggle from "./PreferenceToggle.vue";
import SelectField from "../../../shared/presentation/SelectField.vue";
defineProps<{
  scope?: "wake" | "hotkey" | "command-hotkey";
  advanced?: boolean;
}>();
const draft = defineModel<Preferences>({ required: true });
const phrase = ref("");
const result = ref("");
const phrases = ["Эй, Fono", "Привет, компьютер", "Начни запись"].map(
  (value) => ({ label: value, value }),
);
function test() {
  result.value =
    phrase.value.trim().toLocaleLowerCase("ru") ===
    draft.value.wakePhrase.toLocaleLowerCase("ru")
      ? "Фраза совпала. Fono начнёт диктовку."
      : "Фраза не совпала. Произнесите: «" + draft.value.wakePhrase + "».";
}
</script>
<template>
  <div class="form-stack">
    <template v-if="scope !== 'wake' && scope !== 'command-hotkey'">
      <WlField
        v-slot="field"
        id="hotkey"
        label="Горячая клавиша"
        hint="Например Ctrl + Space или Alt + F9. В демо работает, пока открыта вкладка."
        ><WlInput v-bind="field" v-model="draft.hotkey"
      /></WlField>
    </template>
    <WlField
      v-if="!scope || scope === 'command-hotkey'"
      v-slot="field"
      id="commandHotkey"
      label="Горячая клавиша команд"
      hint="Отдельное сочетание для голосовых команд. В демо открывает проверку фразы."
      ><WlInput v-bind="field" v-model="draft.commandHotkey"
    /></WlField>
    <template v-if="scope !== 'hotkey' && scope !== 'command-hotkey'">
      <PreferenceToggle
        name="wakeEnabled"
        label="Пробуждение голосом"
        description="Начинайте диктовку поддерживаемой фразой."
      />
      <SelectField
        id="wakePhrase"
        v-model="draft.wakePhrase"
        label="Фраза пробуждения"
        :options="phrases"
      />
      <SelectField
        id="silenceMs"
        v-model="draft.silenceMs"
        label="Завершать после паузы"
        :options="[
          { value: 800, label: '0,8 секунды' },
          { value: 1600, label: '1,6 секунды · рекомендуется' },
          { value: 2500, label: '2,5 секунды' },
          { value: 4000, label: '4 секунды' },
        ]"
      />
      <WlField
        v-slot="field"
        label="Проверка фразы"
        hint="Введите фразу, чтобы проверить совпадение."
        ><div class="inline-input">
          <WlInput
            v-bind="field"
            v-model="phrase"
            :placeholder="draft.wakePhrase"
            @keydown.enter.prevent="test"
          /><WlButton :disabled="!phrase.trim()" @click="test"
            >Проверить</WlButton
          >
        </div></WlField
      >
      <p v-if="result" class="notice" role="status">{{ result }}</p>
      <details v-if="advanced" id="advanced" class="advanced">
        <summary>Дополнительно · калибровка и пороги</summary>
        <div class="form-stack">
          <label
            >Чувствительность пробуждения ·
            {{ Math.round(draft.wakeThreshold * 100) }}%<WlSlider
              v-model="draft.wakeThreshold"
              :min="0.1"
              :max="0.9"
              :step="0.05"
              aria-label="Чувствительность пробуждения"
          /></label>
          <label
            >Порог речи ·
            {{ Math.round(draft.speechThreshold * 100) }}%<WlSlider
              v-model="draft.speechThreshold"
              :min="0.1"
              :max="0.9"
              :step="0.05"
              aria-label="Порог речи"
          /></label>
          <WlButton
            size="sm"
            @click="
              draft.wakeThreshold = 0.55;
              draft.speechThreshold = 0.4;
              result =
                'Калибровка выполнена на демонстрационном образце. Сохраните новые пороги.';
            "
            >Калибровать</WlButton
          >
        </div>
      </details>
    </template>
  </div>
</template>
