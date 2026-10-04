<script setup lang="ts">
import { computed, nextTick } from "vue";
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
let checksInput: HTMLInputElement | undefined;
let restoreChecksFocus = false;
function rememberChecksInput(event: FocusEvent) {
  checksInput = event.currentTarget as HTMLInputElement;
}
function syncChecksInput(event: Event) {
  // A refused controlled change leaves Vue's value unchanged; restore the
  // native checkbox without remounting the keyboard focus target.
  checksInput = event.currentTarget as HTMLInputElement;
  restoreChecksFocus ||=
    checksInput.ownerDocument.activeElement === checksInput;
  checksInput.checked = status.checksEnabled;
}
async function changeChecksEnabled(value: boolean) {
  if (busy.value || actionPending.value || status.phase === "not_configured")
    return;
  restoreChecksFocus =
    !!checksInput && checksInput.ownerDocument.activeElement === checksInput;
  try {
    await run(() => port.setChecksEnabled(value));
  } finally {
    await nextTick();
    const document = checksInput?.ownerDocument;
    if (
      restoreChecksFocus &&
      checksInput?.isConnected &&
      !checksInput.disabled &&
      document?.activeElement === document?.body
    )
      checksInput.focus({ preventScroll: true });
    restoreChecksFocus = false;
  }
}
const progress = computed(() => downloadPercent(status));
const recording = computed(
  () =>
    !!workspace.state.pendingDictation ||
    [
      "listening",
      "silence",
      "transcribing",
      "processing",
      "awaiting_action",
    ].includes(workspace.state.phase),
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
    <div class="toggle-row">
      <span
        ><strong>Проверять автоматически</strong
        ><small class="muted"
          >При запуске и затем раз в сутки. Установка — только по вашей
          кнопке.</small
        ></span
      >
      <WlSwitch
        id="update-checks"
        aria-label="Проверять обновления автоматически"
        :model-value="status.checksEnabled"
        :disabled="busy || actionPending || status.phase === 'not_configured'"
        @update:model-value="changeChecksEnabled"
        @change="syncChecksInput"
        @focus="rememberChecksInput"
      />
    </div>
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
