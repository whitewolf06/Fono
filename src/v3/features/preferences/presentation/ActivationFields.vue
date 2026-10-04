<script setup lang="ts">
import { computed } from "vue";
import { WlField, WlInput, WlSlider } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import type { Preferences } from "../../../shared/domain/contracts";
import NativeWakeSetup from "./NativeWakeSetup.vue";
import PreferenceToggle from "./PreferenceToggle.vue";
import SelectField from "../../../shared/presentation/SelectField.vue";
import { LIVE_DICTATION_ENABLED } from "../../../shared/domain/dictationMode";
defineProps<{
  scope?: "wake" | "hotkey" | "command-hotkey";
  advanced?: boolean;
}>();
const workspace = useWorkspace();
const draft = defineModel<Preferences>({ required: true });
const active = computed(() =>
  [
    "listening",
    "silence",
    "transcribing",
    "processing",
    "awaiting_action",
  ].includes(workspace.state.phase),
);
const wakeLanguages = computed(
  () =>
    workspace.state.wakeCapabilities?.languages || [
      { value: "ru", label: "Русский" },
      { value: "en", label: "Английский" },
    ],
);
const phraseOptions = computed(() => {
  const examples =
    draft.value.wakeLanguage === "ru"
      ? ["эй фоно", "привет компьютер"]
      : ["hey fono", "hello computer"];
  const current = draft.value.wakePhrase;
  if (current && !examples.includes(current)) examples.unshift(current);
  return examples.map((value) => ({ value, label: value }));
});
</script>
<template>
  <div class="form-stack">
    <template v-if="scope !== 'wake' && scope !== 'command-hotkey'">
      <SelectField
        v-if="LIVE_DICTATION_ENABLED"
        id="dictationMode"
        v-model="draft.dictationMode"
        label="Режим диктовки"
        :disabled="active"
        :options="[
          { value: 'standard', label: 'Обычная · результат после завершения' },
          {
            value: 'live',
            label: 'Живая · устойчивые фрагменты в выбранное поле',
          },
        ]"
        hint="Живая диктовка работает без ИИ. Паузы разделяют фразы, запись завершается явно."
      />
      <WlField
        v-slot="field"
        id="hotkey"
        label="Горячая клавиша"
        :hint="
          draft.hotkeyMode === 'toggle'
            ? 'Нажмите, чтобы начать в выбранном поле. Нажмите снова, чтобы завершить.'
            : 'Удерживайте сочетание во время речи. Отпустите для распознавания.'
        "
      >
        <WlInput v-bind="field" v-model="draft.hotkey" />
      </WlField>
      <SelectField
        id="hotkeyMode"
        v-model="draft.hotkeyMode"
        label="Как работает горячая клавиша"
        :disabled="active"
        :options="[
          { value: 'hold', label: 'Удерживать · отпустить для завершения' },
          { value: 'toggle', label: 'Нажать · повторное нажатие завершает' },
        ]"
        hint="В обоих вариантах текст появляется после завершения записи. Голосовая команда записывается удержанием своей клавиши."
      />
    </template>
    <WlField
      v-if="!scope || scope === 'command-hotkey'"
      v-slot="field"
      id="commandHotkey"
      label="Горячая клавиша команд"
      :hint="
        workspace.native
          ? 'Удерживайте для записи голосовой команды.'
          : 'В демо открывает проверку фразы.'
      "
    >
      <WlInput v-bind="field" v-model="draft.commandHotkey" />
    </WlField>
    <template v-if="scope !== 'hotkey' && scope !== 'command-hotkey'">
      <PreferenceToggle
        name="wakeEnabled"
        label="Пробуждение голосом"
        description="Локальная активация выбранной фразой. Сначала настройте и проверьте профиль."
      />
      <SelectField
        id="wakeLanguage"
        v-model="draft.wakeLanguage"
        label="Язык фразы пробуждения"
        :options="wakeLanguages"
        hint="Выберите язык своей фразы. Одновременно работает один профиль."
      />
      <SelectField
        v-model="draft.wakePhrase"
        label="Примеры фраз"
        :options="phraseOptions"
        hint="Пример заполняет поле ниже. Новую фразу нужно сохранить и проверить в мастере."
      />
      <p
        v-if="workspace.state.wakePhrases?.length"
        class="muted wake-phrase-catalog"
      >
        Фразы текущего детектора: {{ workspace.state.wakePhrases.join(" · ") }}
      </p>
      <WlField
        v-slot="field"
        id="wakePhrase"
        label="Своя фраза пробуждения"
        :hint="
          draft.wakeLanguage === 'ru'
            ? 'Например «эй фоно» или «привет компьютер». Более различимая фраза уменьшает случайные включения.'
            : 'Например «hey fono» или «hello computer». Фраза проверяется на реальных записях.'
        "
      >
        <WlInput v-bind="field" v-model="draft.wakePhrase" :maxlength="80" />
      </WlField>
      <SelectField
        v-if="draft.dictationMode === 'standard'"
        id="silenceMs"
        v-model="draft.silenceMs"
        label="Завершать после паузы"
        :options="[
          { value: 800, label: '0,8 секунды' },
          { value: 1600, label: '1,6 секунды' },
          { value: 2000, label: '2 секунды' },
          {
            value: draft.silenceMs,
            label: draft.silenceMs / 1000 + ' сек · текущее',
          },
          { value: 2500, label: '2,5 секунды' },
          { value: 4000, label: '4 секунды' },
        ]"
      />
      <p v-else class="notice">
        В живом режиме пауза завершает фрагмент. Вся диктовка продолжается до
        кнопки «Завершить» или повторного нажатия горячей клавиши.
      </p>
      <NativeWakeSetup
        v-if="workspace.wake"
        :phrase="draft.wakePhrase"
        :language="draft.wakeLanguage"
      />
      <details v-if="advanced" id="advanced" class="advanced">
        <summary>Дополнительно · чувствительность</summary>
        <div class="form-stack">
          <p class="muted">
            Мастер подбирает чувствительность по пяти повторам и отдельной
            проверке. Изменение порога требует повторной проверки.
          </p>
          <label
            >Чувствительность пробуждения ·
            {{ Math.round(draft.wakeThreshold * 100) }}%
            <WlSlider
              v-model="draft.wakeThreshold"
              :min="0.1"
              :max="0.9"
              :step="0.05"
              aria-label="Чувствительность пробуждения"
            />
          </label>
          <label
            >Порог речи · {{ Math.round(draft.speechThreshold * 100) }}%
            <WlSlider
              v-model="draft.speechThreshold"
              :min="workspace.native ? 0.001 : 0.1"
              :max="workspace.native ? 0.05 : 0.9"
              :step="workspace.native ? 0.001 : 0.05"
              aria-label="Порог речи"
            />
          </label>
        </div>
      </details>
    </template>
  </div>
</template>
