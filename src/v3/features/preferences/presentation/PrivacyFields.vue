<script setup lang="ts">
import { useFeedback } from "../../../shared/application/feedback";
import { WlButton } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import PreferenceToggle from "./PreferenceToggle.vue";
import SelectField from "../../../shared/presentation/SelectField.vue";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
const draft = defineModel<Preferences>({ required: true });
const workspace = useWorkspace();
const { run } = useFeedback();
const ui = useInteraction();
async function clear(kind: "history" | "trainer") {
  if (
    await ui.confirm({
      title:
        kind === "history" ? "Удалить историю?" : "Удалить данные анализа?",
      text:
        kind === "history"
          ? "Архивные записи и разборы будут удалены. Последний черновик на главной останется."
          : "Исходные расшифровки в архиве и их разборы будут удалены. Итоговые тексты останутся.",
      accept: "Удалить данные",
      danger: true,
    })
  ) {
    await run(() =>
      kind === "history"
        ? workspace.history.clear()
        : workspace.trainer.clear(),
    );
  }
}
</script>
<template>
  <div class="form-stack">
    <PreferenceToggle
      name="historyEnabled"
      label="Сохранять историю диктовок"
      description="Последний результат текущей сессии доступен даже при выключенной истории."
    /><PreferenceToggle
      name="trainerEnabled"
      label="Речевой тренер"
      description="Замечать привычки по исходным расшифровкам."
    /><PreferenceToggle
      name="analyticsConsent"
      label="Локальное хранение исходных расшифровок"
      description="Необходимо для анализа речи. Облачные рекомендации разрешаются отдельно."
    /><SelectField
      id="retentionDays"
      v-model="draft.retentionDays"
      :label="
        workspace.native
          ? 'Срок хранения исходных расшифровок и анализа'
          : 'Срок хранения'
      "
      :options="[
        { value: 7, label: '7 дней' },
        { value: 30, label: '30 дней' },
        { value: 90, label: '90 дней' },
        ...(workspace.native
          ? [{ value: 365, label: '365 дней' }]
          : [{ value: 0, label: 'До ручного удаления' }]),
      ]"
    />
    <div v-if="workspace.native" class="notice">
      История хранится локально на компьютере. Итоговые тексты остаются до
      ручного удаления. Исходные расшифровки и анализ удаляются по выбранному
      сроку. Ключи подключений защищены хранилищем Windows.
    </div>
    <div v-else class="notice">
      В этом браузерном макете сохраняются только демонстрационные настройки.
      Тексты, ключи и пользовательские инструкции не записываются в хранилище
      браузера.
    </div>
    <section class="danger-zone">
      <h3>Накопленные данные</h3>
      <p>
        {{ workspace.state.history.length }} диктовок ·
        {{ workspace.state.history.filter((e) => e.original).length }} исходных
        расшифровок
      </p>
      <div class="actions">
        <WlButton
          size="sm"
          variant="danger-quiet"
          :disabled="!workspace.state.history.length"
          @click="clear('history')"
          >Удалить историю</WlButton
        ><WlButton
          size="sm"
          variant="danger-quiet"
          :disabled="!workspace.state.history.some((e) => e.original)"
          @click="clear('trainer')"
          >Удалить данные анализа</WlButton
        >
      </div>
    </section>
  </div>
</template>
