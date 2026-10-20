<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { WlButton, WlField, WlTextarea } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import SelectionDelivery from "./SelectionDelivery.vue";
const workspace = useWorkspace();
const current = computed(() => workspace.selection.state.current);
const edit = computed(() => current.value?.edit);
const instruction = ref("");
watch(
  [() => current.value?.operationId, () => edit.value?.instruction],
  () => {
    instruction.value = edit.value?.instruction ?? "";
  },
  { immediate: true },
);
const active = computed(
  () =>
    Boolean(edit.value?.operationId) ||
    ["replacing", "copying", "cancelling"].includes(
      current.value?.delivery?.phase ?? "idle",
    ),
);
const { run, busy, error } = useFeedback();
function start() {
  const id = current.value?.operationId;
  if (id) void run(() => workspace.selection.startInstruction(id));
}
function stop() {
  const id = current.value?.operationId,
    job = edit.value?.operationId;
  if (id && job) void run(() => workspace.selection.stopInstruction(id, job));
}
function cancel() {
  const id = current.value?.operationId,
    job = edit.value?.operationId;
  if (id && job) void run(() => workspace.selection.cancelEdit(id, job));
}
function transform() {
  const id = current.value?.operationId;
  if (id)
    void run(() => workspace.selection.transform(id, instruction.value, true));
}
const status = computed(
  () =>
    ({
      starting: "Открываю микрофон для инструкции…",
      recording:
        "Записываю инструкцию. Нажмите «Завершить запись», когда закончите.",
      recognizing: "Распознаю инструкцию локально…",
      processing: "Преобразую выделение через выбранное LLM-подключение…",
      cancelling: "Отменяю действие и завершаю фоновую работу…",
      idle: "",
      ready: "",
    })[edit.value?.phase ?? "idle"],
);
</script>
<template>
  <section class="form-stack">
    <WlField
      id="selectionInstruction"
      v-slot="field"
      label="Инструкция к выделению"
    >
      <WlTextarea
        v-bind="field"
        v-model="instruction"
        :disabled="active || busy"
        :rows="3"
        :maxlength="4096"
        placeholder="Например: сделай формальнее или переведи на английский"
      />
    </WlField>
    <p class="muted">
      Инструкция хранится отдельно от оригинала. Можно продиктовать её или
      ввести вручную.
    </p>
    <div class="actions">
      <WlButton
        :disabled="active || busy"
        @click="instruction = 'Сделай формальнее'"
        >Формальнее</WlButton
      >
      <WlButton
        :disabled="active || busy"
        @click="instruction = 'Переведи на английский'"
        >На английский</WlButton
      >
      <WlButton
        :disabled="active || busy"
        @click="instruction = 'Сократи до трёх пунктов'"
        >В три пункта</WlButton
      >
    </div>
    <div class="actions">
      <WlButton
        v-if="edit?.phase === 'recording' || edit?.phase === 'starting'"
        :loading="busy"
        @click="stop"
        >Завершить запись</WlButton
      >
      <WlButton v-else :disabled="active || busy" @click="start">{{
        workspace.native
          ? "Продиктовать инструкцию"
          : "Пример голосовой инструкции"
      }}</WlButton>
      <WlButton
        v-if="active"
        :disabled="edit?.phase === 'cancelling' || busy"
        @click="cancel"
        >Отменить действие</WlButton
      >
    </div>
    <p v-if="status" role="status">{{ status }}</p>
    <p v-if="workspace.native" class="notice">
      По кнопке «Преобразовать в LLM» выделение и инструкция отправятся в
      сохранённое подключение обработки текста. Если оно облачное, текст получит
      его провайдер. Отмена не возвращает уже отправленные данные.
    </p>
    <p v-else class="notice">
      В демо используются готовые примеры. Микрофон и сеть не используются.
    </p>
    <WlButton
      :disabled="active || busy || !instruction.trim()"
      @click="transform"
      >{{
        workspace.native ? "Преобразовать в LLM" : "Показать деморезультат"
      }}</WlButton
    >
    <p v-if="error || edit?.message" role="alert" class="error-text">
      {{ error || edit?.message }}
    </p>
    <template v-if="edit?.result">
      <h3>Результат преобразования</h3>
      <WlTextarea
        :model-value="edit.result"
        readonly
        :rows="6"
        aria-label="Результат преобразования выделения"
      />
      <p class="muted">
        Оригинал доступен выше. Замена выполняется только после подтверждения.
      </p>
      <SelectionDelivery
        :instruction-matches="instruction === edit.instruction"
      />
    </template>
  </section>
</template>
