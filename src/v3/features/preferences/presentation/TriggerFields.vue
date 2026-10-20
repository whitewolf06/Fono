<script setup lang="ts">
import { computed, ref } from "vue";
import { WlButton, WlField, WlInput } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import { triggerConflict } from "../../../shared/domain/triggers";
import { useWorkspace } from "../../../shared/application/workspace";
import { useTriggerCapture } from "../application/useTriggerCapture";
import SelectField from "../../../shared/presentation/SelectField.vue";
const props = defineProps<{
  scope?: "hotkey" | "command-hotkey";
  active: boolean;
}>();
const draft = defineModel<Preferences>({ required: true });
const workspace = useWorkspace();
const target = ref<"dictation" | "command">("dictation");
const captureError = ref("");
const { capturing, message, start, cancel } = useTriggerCapture((candidate) => {
  captureError.value = "";
  if (target.value === "command") {
    if (candidate.trigger !== "shortcut" || !candidate.hotkey) {
      captureError.value =
        "Для голосовых команд нужно сочетание Ctrl/Alt/Shift с другой клавишей.";
      return;
    }
    draft.value.commandHotkey = candidate.hotkey;
  } else {
    draft.value.dictationTrigger = candidate.trigger;
    if (candidate.hotkey) draft.value.hotkey = candidate.hotkey;
  }
});
const conflict = computed(() => triggerConflict(draft.value));
const disabled = computed(() => props.active || capturing.value);
const keyboardHook = computed(() =>
  ["right_alt", "right_ctrl", "caps_lock"].includes(
    draft.value.dictationTrigger,
  ),
);
const behavior = computed(() =>
  draft.value.hotkeyMode === "hold"
    ? "Удерживайте кнопку во время речи. Отпустите для распознавания."
    : keyboardHook.value
      ? "Дважды нажмите кнопку, чтобы начать. Дважды нажмите снова, чтобы завершить. Интервал — до 350 мс."
      : "Нажмите, чтобы начать. Нажмите снова, чтобы завершить.",
);
function choose(value: "dictation" | "command") {
  target.value = value;
  captureError.value = "";
  void start();
}
</script>
<template>
  <div class="form-stack">
    <template v-if="scope !== 'command-hotkey'">
      <SelectField
        id="dictationTrigger"
        v-model="draft.dictationTrigger"
        label="Кнопка диктовки"
        :disabled="disabled"
        :options="[
          { value: 'shortcut', label: 'Сочетание клавиш · по умолчанию' },
          { value: 'right_alt', label: 'Правый Alt' },
          { value: 'right_ctrl', label: 'Правый Ctrl' },
          { value: 'caps_lock', label: 'CapsLock' },
          { value: 'x_button1', label: 'Боковая кнопка мыши 1 · назад' },
          { value: 'x_button2', label: 'Боковая кнопка мыши 2 · вперёд' },
        ]"
      />
      <WlField
        v-if="draft.dictationTrigger === 'shortcut'"
        id="hotkey"
        v-slot="field"
        label="Горячая клавиша"
        :hint="behavior"
      >
        <WlInput v-bind="field" v-model="draft.hotkey" :disabled="disabled" />
      </WlField>
      <p v-else class="muted">{{ behavior }}</p>
      <WlButton :disabled="disabled" @click="choose('dictation')"
        >Выбрать кнопку нажатием</WlButton
      >
      <SelectField
        id="hotkeyMode"
        v-model="draft.hotkeyMode"
        label="Как работает кнопка"
        :disabled="disabled"
        :options="[
          { value: 'hold', label: 'Удерживать · отпустить для завершения' },
          {
            value: 'toggle',
            label: keyboardHook
              ? 'Дважды нажать · повторить для завершения'
              : 'Нажать · повторное нажатие завершает',
          },
        ]"
        hint="В обоих вариантах текст появляется после завершения записи."
      />
      <p v-if="draft.dictationTrigger === 'right_alt'" class="notice">
        Правый Alt также используется как AltGr для ввода символов. Выберите
        другую кнопку, если она нужна в вашей раскладке.
      </p>
      <p v-if="draft.dictationTrigger === 'caps_lock'" class="notice">
        Пока CapsLock управляет диктовкой, нажатия не переключают регистр.
      </p>
      <p v-if="draft.dictationTrigger.startsWith('x_button')" class="notice">
        Выбранная боковая кнопка будет управлять диктовкой вместо перехода назад
        или вперёд.
      </p>
    </template>
    <WlField
      id="commandHotkey"
      v-slot="field"
      label="Горячая клавиша команд"
      hint="Удерживайте сочетание для записи голосовой команды. Оно не должно включать выбранный правый модификатор."
    >
      <WlInput
        v-bind="field"
        v-model="draft.commandHotkey"
        :disabled="disabled"
      />
    </WlField>
    <WlButton :disabled="disabled" @click="choose('command')"
      >Выбрать сочетание команд</WlButton
    >
    <p v-if="capturing" class="notice" role="status">{{ message }}</p>
    <WlButton v-if="capturing" @click="cancel">Отменить выбор</WlButton>
    <p v-else-if="message" class="muted" role="status">{{ message }}</p>
    <p v-if="conflict || captureError" class="error-text" role="alert">
      {{ captureError || conflict }}
    </p>
    <p v-if="active" class="notice">
      Сначала завершите диктовку. Кнопки можно менять между сеансами.
    </p>
    <p class="muted">
      Fono проверяет свои конфликты. Занятость сочетания в Windows проверяется
      при сохранении; сочетания других приложений могут пересекаться. Если
      сохранение не удалось, проверьте сообщение об ошибке перед повтором.
    </p>
    <details v-if="scope !== 'command-hotkey'" class="advanced">
      <summary>Педаль и USB-кнопка</summary>
      <p class="muted">
        Настройте устройство на физическое нажатие одной из поддерживаемых
        кнопок или сочетания. Программы, которые имитируют ввод, могут
        отфильтровываться хуком. Для AutoHotkey, Stream Deck и подобных программ
        используйте запуск <code>Fono.exe --start</code>, <code>--stop</code>,
        <code>--toggle</code> или <code>--cancel</code>. Это команды
        установленного Fono; в этой форме они не запускаются.
      </p>
    </details>
    <p v-if="!workspace.native" class="notice">
      В браузере проверяется только форма выбора. Глобальные кнопки, мышь и
      USB-устройства проверяются в Windows-приложении Fono.
    </p>
  </div>
</template>
