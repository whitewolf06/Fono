<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import type { SelectionCapturePort } from "../domain/selection";
import { useFeedback } from "../../../shared/application/feedback";
import { cancelSelection } from "../application/cancelSelection";
const props = defineProps<{
  port: SelectionCapturePort;
  preferences: Preferences;
  native: boolean;
}>();
const current = computed(() => props.port.state.current);
const edit = computed(() => current.value?.edit);
const delivery = computed(() => current.value?.delivery);
const instruction = ref("");
watch(
  [() => current.value?.operationId, () => edit.value?.instruction],
  () => {
    instruction.value = edit.value?.instruction ?? "";
  },
  { immediate: true },
);
const { run, busy, error } = useFeedback();
const working = computed(
  () =>
    !!edit.value?.operationId ||
    ["replacing", "copying", "cancelling"].includes(
      delivery.value?.phase ?? "idle",
    ),
);
function action(
  kind: "record" | "stop" | "transform" | "insert" | "copy" | "cancel",
) {
  const s = current.value;
  if (!s) return;
  void run(async () => {
    if (kind === "cancel") return cancelSelection(props.port);
    if (kind === "record") return props.port.startInstruction(s.operationId);
    if (kind === "stop" && edit.value?.operationId)
      return props.port.stopInstruction(s.operationId, edit.value.operationId);
    if (kind === "transform")
      return props.port.transform(s.operationId, instruction.value, true);
    const result = s.edit?.resultId;
    if (!result) return;
    if (kind === "insert")
      return props.port.insertResult(s.operationId, result);
    if (kind === "copy") return props.port.copyResult(s.operationId, result);
  });
}
const status = computed(
  () =>
    ({
      starting: "Открываю микрофон для инструкции…",
      recording: "Записываю инструкцию…",
      recognizing: "Распознаю инструкцию локально…",
      processing: "Преобразую выделение в LLM…",
      cancelling: "Завершаю отменённую работу…",
      idle: "",
      ready: "",
    })[edit.value?.phase ?? "idle"],
);
</script>
<template>
  <div v-if="current" class="overlay-preview-wrap">
    <section
      class="overlay-indicator overlay-preview selection-overlay"
      aria-label="Правка выделения"
      data-overlay-interactive
      :style="{
        zoom: preferences.overlayScale / 100,
        '--fono-overlay-zoom': preferences.overlayScale / 100,
        opacity: preferences.overlayOpacity / 100,
      }"
    >
      <div class="section-header">
        <h3>Правка выделения</h3>
        <WlButton :disabled="busy" size="sm" @click="action('cancel')"
          >Отмена</WlButton
        >
      </div>
      <p v-if="current.phase === 'reading'" role="status">
        Получаю выделенный фрагмент…
      </p>
      <p v-else-if="current.phase === 'error'" role="alert">
        {{ current.message }}
      </p>
      <template v-else>
        <details>
          <summary>Оригинал</summary>
          <p class="selection-overlay-text">{{ current.text }}</p>
        </details>
        <p class="selection-overlay-text" aria-label="Инструкция к выделению">
          <strong>Инструкция:</strong>
          {{ instruction || "Продиктуйте или выберите вариант ниже." }}
        </p>
        <div class="actions">
          <WlButton
            size="sm"
            :disabled="working || busy"
            @click="instruction = 'Сделай формальнее'"
            >Формальнее</WlButton
          >
          <WlButton
            size="sm"
            :disabled="working || busy"
            @click="instruction = 'Переведи на английский'"
            >На английский</WlButton
          >
          <WlButton
            size="sm"
            :disabled="working || busy"
            @click="instruction = 'Сократи до трёх пунктов'"
            >В три пункта</WlButton
          >
        </div>
        <div class="actions">
          <WlButton
            v-if="edit?.phase === 'starting' || edit?.phase === 'recording'"
            :disabled="busy"
            @click="action('stop')"
            >Завершить запись</WlButton
          >
          <WlButton
            v-else
            :disabled="working || busy"
            @click="action('record')"
            >{{
              native ? "Продиктовать инструкцию" : "Пример голосовой инструкции"
            }}</WlButton
          >
          <WlButton
            :disabled="working || busy || !instruction.trim()"
            @click="action('transform')"
            >{{
              native ? "Преобразовать в LLM" : "Показать деморезультат"
            }}</WlButton
          >
        </div>
        <p v-if="status" role="status">{{ status }}</p>
        <p class="muted">
          {{
            native
              ? "По кнопке «Преобразовать в LLM» выделение и инструкция уйдут в выбранное подключение. Для облачного подключения текст получит провайдер; отмена не отзывает отправленные данные."
              : "Демонстрация: микрофон, сеть, чужие поля и системный буфер не используются."
          }}
        </p>
        <p class="muted">
          {{
            current.autoInsert
              ? "Выбрана вставка сразу после LLM с проверкой исходного поля."
              : "Замена ждёт кнопки «Вставить»."
          }}
          {{
            current.historyEnabled
              ? "В историю сохранится только принятый результат, если сохранение остаётся включено."
              : "Сохранение правки в историю выключено."
          }}
        </p>
        <template v-if="edit?.result">
          <p class="selection-overlay-text" aria-label="Результат правки">
            {{ edit.result }}
          </p>
          <div class="actions">
            <WlButton
              :disabled="
                working ||
                busy ||
                !delivery?.replaceAllowed ||
                instruction !== edit.instruction
              "
              @click="action('insert')"
              >Вставить</WlButton
            >
            <WlButton :disabled="working || busy" @click="action('copy')"
              >Копировать</WlButton
            >
          </div>
          <p v-if="delivery?.phase === 'submitted'" role="status">
            {{
              native
                ? "Текст отправлен. Проверьте исходное поле; повторная вставка отключена."
                : "Демонстрационная замена завершена. Другие приложения не использовались."
            }}
          </p>
          <p v-if="delivery?.phase === 'copied'" role="status">
            {{
              native
                ? "Результат скопирован."
                : "Демонстрация копирования: системный буфер не менялся."
            }}
          </p>
          <p
            v-if="['blocked', 'uncertain'].includes(delivery?.phase ?? '')"
            role="alert"
          >
            Замена не подтверждена. Проверьте поле перед ручной вставкой.
            Результат доступен для копирования; повтор отключён.
          </p>
          <p
            v-if="
              ['replacing', 'copying', 'cancelling'].includes(
                delivery?.phase ?? '',
              )
            "
            role="status"
          >
            Завершаю действие…
          </p>
        </template>
      </template>
      <p
        v-if="error || edit?.message || delivery?.message"
        role="alert"
        class="error-text"
      >
        {{ error || edit?.message || delivery?.message }}
      </p>
      <p v-if="current.historyMessage" role="alert" class="error-text">
        {{ current.historyMessage }}
      </p>
    </section>
  </div>
</template>
