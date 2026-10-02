<script setup lang="ts">
import { ref } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
const workspace = useWorkspace();
const { run, busy, error } = useFeedback();
const status = ref("");
async function action(name: string) {
  await run(async () => {
    status.value = await workspace.wake!.action(name);
  });
}
</script>
<template>
  <section v-if="workspace.wake" class="form-stack">
    <p class="muted">
      Сохраните выбранную фразу. Затем запишите образцы и проверьте активацию.
      Запись запускается только кнопкой.
    </p>
    <div class="actions">
      <WlButton size="sm" :loading="busy" @click="action('download')"
        >Загрузить модель пробуждения</WlButton
      >
      <WlButton size="sm" :disabled="busy" @click="action('test')"
        >Проверить фразу голосом</WlButton
      >
    </div>
    <details class="advanced">
      <summary>Калибровка и контрольные записи</summary>
      <div class="form-stack">
        <p>
          Для калибровки произнесите выбранную фразу после нажатия «Записать
          образец». Повторяйте до завершения.
        </p>
        <div class="actions">
          <WlButton size="sm" :disabled="busy" @click="action('start')"
            >Начать калибровку</WlButton
          >
          <WlButton size="sm" :loading="busy" @click="action('record')"
            >Записать образец</WlButton
          >
          <WlButton size="sm" :disabled="busy" @click="action('cancel')"
            >Отменить калибровку</WlButton
          >
        </div>
        <p>
          Затем проверьте профиль свежими записями: фраза пробуждения, тишина и
          другая фраза.
        </p>
        <div class="actions">
          <WlButton size="sm" :disabled="busy" @click="action('validate')"
            >Начать проверку</WlButton
          >
          <WlButton size="sm" :disabled="busy" @click="action('positive')"
            >Произнести фразу</WlButton
          >
          <WlButton size="sm" :disabled="busy" @click="action('silence')"
            >Записать тишину</WlButton
          >
          <WlButton size="sm" :disabled="busy" @click="action('other_phrase')"
            >Другая фраза</WlButton
          >
        </div>
      </div>
    </details>
    <p v-if="busy" role="status">Идёт запись или проверка…</p>
    <p v-if="status" class="notice" role="status">{{ status }}</p>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
  </section>
</template>
