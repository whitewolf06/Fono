<script setup lang="ts">
import { computed } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
defineProps<{ instructionMatches: boolean }>();
const workspace = useWorkspace();
const current = computed(() => workspace.selection.state.current);
const delivery = computed(() => current.value?.delivery);
const active = computed(() =>
  ["replacing", "copying", "cancelling"].includes(
    delivery.value?.phase ?? "idle",
  ),
);
const { run, busy, error } = useFeedback();
function action(kind: "arm" | "copy" | "cancel") {
  const id = current.value?.operationId,
    result = current.value?.edit?.resultId;
  if (!id || !result) return;
  void run(() =>
    kind === "arm"
      ? workspace.selection.armReplacement(id, result)
      : kind === "copy"
        ? workspace.selection.copyResult(id, result)
        : workspace.selection.cancelDelivery(id, result),
  );
}
function demo() {
  if (workspace.selection.demoReplace)
    void run(workspace.selection.demoReplace);
}
</script>
<template>
  <section v-if="current?.edit?.resultId" class="form-stack">
    <div class="actions">
      <WlButton
        :disabled="
          busy ||
          active ||
          !delivery?.replaceAllowed ||
          !instructionMatches ||
          Boolean(current.edit.operationId)
        "
        @click="action('arm')"
        >Подтвердить замену</WlButton
      >
      <WlButton
        :disabled="busy || active || Boolean(current.edit.operationId)"
        @click="action('copy')"
        >Копировать результат</WlButton
      >
      <WlButton
        v-if="delivery?.phase === 'armed' || active"
        :disabled="busy || delivery?.phase === 'cancelling'"
        @click="action('cancel')"
        >Отменить замену</WlButton
      >
    </div>
    <p v-if="delivery?.phase === 'armed'" role="status" class="notice">
      Вернитесь в исходное поле с тем же выделением и нажмите
      {{ workspace.state.preferences.selectionHotkey }}. При изменении поля или
      фрагмента замена отклонится.
    </p>
    <WlButton
      v-if="
        !workspace.native &&
        workspace.selection.demoReplace &&
        delivery?.phase === 'armed'
      "
      @click="demo"
      >Смоделировать замену</WlButton
    >
    <p
      v-if="delivery?.phase === 'replacing' || delivery?.phase === 'cancelling'"
      role="status"
    >
      Завершаю замену. Повторная отправка недоступна.
    </p>
    <p v-if="delivery?.phase === 'copying'" role="status">Копирую результат…</p>
    <p v-if="delivery?.phase === 'submitted'" role="status">
      {{
        workspace.native
          ? "Текст отправлен в исходное поле. Проверьте результат; повторная замена здесь отключена."
          : "Демонстрационная замена завершена. Другие приложения не использовались."
      }}
    </p>
    <p v-if="delivery?.phase === 'copied'" role="status">
      {{
        workspace.native
          ? "Результат скопирован."
          : "Демонстрация копирования. Системный буфер не менялся."
      }}
    </p>
    <p
      v-if="delivery?.phase === 'blocked' || delivery?.phase === 'uncertain'"
      role="alert"
      class="notice"
    >
      Замена не подтверждена. Проверьте исходное поле перед ручной вставкой.
      Результат доступен для копирования; повтор отключён.
    </p>
    <p v-if="delivery?.message || error" role="alert" class="error-text">
      {{ error || delivery?.message }}
    </p>
  </section>
</template>
