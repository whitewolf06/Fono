<script setup lang="ts">
import { computed } from "vue";
import { WlButton, WlSwitch } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
import { useFeedback } from "../../../shared/application/feedback";
import { updateBusy, downloadPercent } from "../../../shared/domain/updates";
const props = defineProps<{ unsaved: boolean }>();
const workspace = useWorkspace();
const ui = useInteraction();
const { run, error, busy: actionPending } = useFeedback();
const port = workspace.updates;
const status = port.state;
const busy = computed(() => updateBusy(status.phase));
const progress = computed(() => downloadPercent(status));
const recording = computed(() =>
  ["listening", "silence", "transcribing", "processing"].includes(
    workspace.state.phase,
  ),
);
async function install() {
  if (props.unsaved || recording.value) return;
  const approved = await ui.confirm({
    title: `Установить Fono ${status.nextVersion}?`,
    text: "После загрузки и проверки подписи Fono закроется для установки. Windows может запросить разрешение администратора. Сначала сохраните последний текст: черновик текущей сессии не переносится после перезапуска.",
    accept: "Скачать и установить",
  });
  if (approved) await run(() => port.install());
}
</script>
<template>
  <section
    id="updates"
    class="form-stack update-settings"
    aria-label="Обновления Fono"
  >
    <div class="section-header">
      <h3>Обновления</h3>
      <small class="muted">Установлено {{ status.currentVersion }}</small>
    </div>
    <p aria-live="polite">{{ status.message }}</p>
    <p
      v-if="
        status.nextVersion &&
        ['available', 'downloading'].includes(status.phase)
      "
      class="notice"
    >
      Новая версия: <strong>{{ status.nextVersion }}</strong>
    </p>
    <label class="toggle-row" for="update-checks">
      <span
        ><strong>Проверять при запуске</strong
        ><small class="muted"
          >Только проверка новой версии. Установка — по вашей кнопке.</small
        ></span
      >
      <WlSwitch
        id="update-checks"
        :model-value="status.checksEnabled"
        :disabled="busy || actionPending || status.phase === 'not_configured'"
        @update:model-value="run(() => port.setChecksEnabled($event))"
      />
    </label>
    <div v-if="status.phase === 'downloading'" class="form-stack">
      <progress
        :value="progress ?? undefined"
        :max="100"
        aria-label="Загрузка обновления"
      />
      <small class="muted">{{
        progress === null ? "Загрузка…" : `${progress}% · загрузка обновления`
      }}</small>
    </div>
    <p v-if="props.unsaved || recording" class="notice">
      Перед установкой завершите диктовку и сохраните настройки.
    </p>
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    <div class="actions">
      <WlButton
        size="sm"
        :loading="status.phase === 'checking'"
        :disabled="busy || actionPending || status.phase === 'not_configured'"
        @click="run(() => port.check())"
        >Проверить обновления</WlButton
      >
      <WlButton
        v-if="status.phase === 'available'"
        size="sm"
        variant="primary"
        :disabled="props.unsaved || recording || actionPending"
        @click="install"
        >Скачать и установить</WlButton
      >
      <WlButton
        v-if="status.phase === 'downloading'"
        size="sm"
        @click="run(() => port.cancel())"
        >Отменить загрузку</WlButton
      >
    </div>
    <small v-if="!workspace.native" class="muted"
      >Браузерный макет: без подключения к GitHub и без установки.</small
    >
  </section>
</template>
<style scoped>
.update-settings {
  border-top: 1px solid var(--fono-border);
  padding-top: var(--fono-space-4);
}
.toggle-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--fono-space-3);
}
.toggle-row small {
  display: block;
  margin-top: var(--fono-space-1);
}
progress {
  width: 100%;
  accent-color: var(--fono-accent);
}
</style>
