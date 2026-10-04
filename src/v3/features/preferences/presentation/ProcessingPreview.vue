<script setup lang="ts">
import { WlButton, WlField, WlTextarea } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import { useProcessingPreview } from "../application/useProcessingPreview";
const draft = defineModel<Preferences>({ required: true });
const {
  workspace,
  input,
  original,
  result,
  busy,
  capturing,
  captureBusy,
  error,
  notice,
  usesModel,
  connectionChanged,
  connectionMissing,
  activeDictation,
  canTest,
  test,
  useLast,
  startCapture,
  finishCapture,
  cancelCapture,
  copy,
} = useProcessingPreview(draft);
</script>
<template>
  <section class="processing-preview" aria-labelledby="processing-test-heading">
    <header>
      <h3 id="processing-test-heading">Проверить на своём тексте</h3>
      <p class="muted">
        Проверка использует промпт из формы. Текст не вставляется в другие окна
        и не попадает в историю.
      </p>
    </header>
    <p v-if="!workspace.native" class="notice">
      В браузере показан демонстрационный результат. Настоящая модель и микрофон
      доступны в приложении.
    </p>
    <WlField v-slot="field" label="Исходный текст"
      ><WlTextarea
        v-bind="field"
        v-model="input"
        :rows="4"
        :disabled="capturing || captureBusy || busy"
        placeholder="Напишите текст или надиктуйте пробный фрагмент."
    /></WlField>
    <div class="processing-preview-actions">
      <WlButton
        size="sm"
        :disabled="
          capturing ||
          captureBusy ||
          busy ||
          (!workspace.state.last.entry && !workspace.state.last.draft)
        "
        @click="useLast"
        ><AppIcon name="clock" :size="15" />Последняя диктовка</WlButton
      >
      <template v-if="capturing"
        ><WlButton size="sm" :loading="captureBusy" @click="finishCapture"
          ><AppIcon name="check" :size="15" />Завершить диктовку</WlButton
        ><WlButton size="sm" :disabled="captureBusy" @click="cancelCapture"
          >Отменить</WlButton
        ></template
      >
      <WlButton
        v-else
        size="sm"
        :loading="captureBusy"
        :disabled="busy || activeDictation"
        @click="startCapture"
        ><AppIcon name="microphone" :size="15" />{{
          workspace.native ? "Надиктовать пример" : "Пробная диктовка · демо"
        }}</WlButton
      >
      <WlButton
        variant="primary"
        size="sm"
        :loading="busy"
        :disabled="!canTest"
        @click="test"
        ><AppIcon name="sparkle" :size="15" />Проверить обработку</WlButton
      >
    </div>
    <p v-if="capturing" class="notice" role="status">
      Слушаю пример. Нажмите «Завершить диктовку», чтобы получить исходный
      текст.
    </p>
    <p v-if="connectionChanged" class="notice">
      Сначала сохраните выбранные подключение и модель. Сам промпт можно
      проверять до сохранения.
    </p>
    <p v-else-if="connectionMissing" class="notice">
      Выберите подключение и модель, затем сохраните настройки.
    </p>
    <p v-else-if="usesModel" class="muted">
      Модель: {{ workspace.state.preferences.processingModel }} · используется
      сохранённое подключение.
    </p>
    <p v-else class="muted">
      «Без изменений» без перевода: текст возвращается сразу, запрос к ИИ не
      отправляется.
    </p>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    <p v-if="notice" class="notice" role="status">{{ notice }}</p>
    <section
      v-if="result"
      class="processing-preview-output"
      aria-label="Результат проверки"
    >
      <div>
        <strong>Исходный текст</strong>
        <pre>{{ original }}</pre>
      </div>
      <div>
        <header>
          <strong>Результат обработки</strong
          ><WlButton size="sm" @click="copy"
            ><AppIcon name="copy" :size="15" />Копировать</WlButton
          >
        </header>
        <pre>{{ result.text }}</pre>
      </div>
      <small
        >{{ result.model || "Без обращения к ИИ" }} ·
        {{
          (result.elapsedMs / 1000).toLocaleString("ru-RU", {
            maximumFractionDigits: 2,
          })
        }}
        с</small
      >
    </section>
  </section>
</template>
