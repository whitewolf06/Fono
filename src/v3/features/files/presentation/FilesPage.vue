<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { WlButton, WlDialog } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import type { AddFilesResult, FileExportFormat } from "../domain/files";
const workspace = useWorkspace();
const jobs = computed(() => workspace.state.jobs.filter((job) => job.file));
const active = computed(() =>
  jobs.value.filter((job) => ["queued", "running"].includes(job.state)),
);
const selectedId = ref<string | null>(null);
const selected = computed(() =>
  jobs.value.find((job) => job.id === selectedId.value),
);
const visible = computed({
  get: () => !!selected.value,
  set: () => {
    selectedId.value = null;
  },
});
const busy = ref(false),
  hovering = ref(false),
  error = ref(""),
  notice = ref("");
const labels = {
  queued: "В очереди",
  running: "Распознавание",
  done: "Готово",
  error: "Ошибка",
  cancelled: "Отменено",
};
function openResult(id: string) {
  error.value = "";
  notice.value = "";
  selectedId.value = id;
}
function report(value: unknown) {
  error.value =
    value instanceof Error
      ? value.message
      : "Не удалось выполнить действие. Повторите попытку.";
}
function added(result: AddFilesResult) {
  notice.value = result.accepted.length
    ? `Добавлено файлов: ${result.accepted.length}.`
    : "";
  if (result.rejected)
    error.value = `Не добавлено: ${result.rejected}. В очереди до 4 ожидающих файлов. Поддерживаются WAV, MP3, FLAC и OGG.`;
}
async function act(action: () => Promise<unknown>) {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  notice.value = "";
  try {
    await action();
  } catch (value) {
    report(value);
  } finally {
    busy.value = false;
  }
}
async function save(format: FileExportFormat) {
  if (!selected.value) return;
  const id = selected.value.id;
  await act(async () => {
    if (await workspace.files.export(id, format))
      notice.value = "Файл сохранён.";
  });
}
let disposed = false,
  release = () => {};
onMounted(() => {
  void workspace.files
    .bindDrop({
      hover: (value) => {
        hovering.value = value;
      },
      added,
      error: report,
    })
    .then((unbind) => {
      if (disposed) unbind();
      else release = unbind;
    })
    .catch(report);
});
onUnmounted(() => {
  disposed = true;
  release();
});
</script>
<template>
  <div class="page files-page">
    <PageHeading
      title="Файлы"
      description="Распознайте аудиозапись и сохраните текст или субтитры."
    >
      <WlButton
        :loading="busy"
        @click="act(async () => added(await workspace.files.choose()))"
        ><AppIcon name="plus" />{{
          workspace.native ? "Добавить файлы" : "Добавить демофайлы"
        }}</WlButton
      >
    </PageHeading>
    <p v-if="!workspace.native" class="notice">
      Демо интерфейса: файлы и результаты условные. Распознавание работает в
      приложении Fono.
    </p>
    <div class="files-drop" :class="{ hovering }">
      <AppIcon name="file" :size="28" />
      <strong>Перетащите сюда аудиофайлы</strong>
      <span>WAV, MP3, FLAC, OGG · до 100 МиБ и 3 часов на файл</span>
    </div>
    <p class="muted files-hint">
      Файлы обрабатываются по одному выбранной моделью. Диктовка получает
      приоритет. ИИ-обработка текста не применяется.
    </p>
    <p v-if="error" class="notice error" role="alert">{{ error }}</p>
    <p v-if="notice" class="notice" role="status">{{ notice }}</p>
    <div class="section-header">
      <h2>
        Очередь и результаты <span class="muted">{{ jobs.length }}</span>
      </h2>
      <WlButton
        v-if="active.length"
        severity="secondary"
        size="sm"
        :disabled="busy"
        @click="
          act(async () => {
            for (const job of active) await workspace.files.cancel(job.id);
          })
        "
        >Отменить все</WlButton
      >
    </div>
    <p v-if="!jobs.length" class="muted files-empty">
      Добавьте несколько записей. Готовые результаты появятся здесь, даже если
      API-сервис выключен.
    </p>
    <ol v-else class="files-list">
      <li v-for="job in jobs" :key="job.id" class="files-row">
        <AppIcon name="file" />
        <div class="file-content">
          <strong>{{ job.name }}</strong
          ><span class="muted"
            >{{ job.preparing ? "Подготовка аудио" : labels[job.state]
            }}<template v-if="job.seconds">
              · {{ Math.floor(job.seconds / 60) }} мин
              {{ job.seconds % 60 }} с</template
            ></span
          >
          <progress
            v-if="job.state === 'running' && !job.preparing"
            :value="job.progress || 0"
            max="1"
            :aria-label="`Прогресс: ${job.name}`"
          />
          <p v-if="job.error" class="file-error">{{ job.error }}</p>
        </div>
        <WlButton
          v-if="['queued', 'running'].includes(job.state)"
          size="sm"
          severity="secondary"
          :disabled="busy"
          @click="act(() => workspace.files.cancel(job.id))"
          >Отменить</WlButton
        >
        <WlButton
          v-else-if="job.state === 'done'"
          size="sm"
          severity="secondary"
          @click="openResult(job.id)"
          >Результат</WlButton
        >
      </li>
    </ol>
    <WlDialog
      v-model:visible="visible"
      :header="selected?.name || 'Результат'"
      width="560px"
    >
      <p class="file-result">{{ selected?.text || "Речь не обнаружена." }}</p>
      <p v-if="selected && !selected.hasTimestamps" class="muted">
        У этого результата нет временных меток. Доступен экспорт TXT.
      </p>
      <p v-if="error" class="notice error" role="alert">{{ error }}</p>
      <p v-if="notice" class="notice" role="status">{{ notice }}</p>
      <template #footer>
        <WlButton
          size="sm"
          severity="secondary"
          :disabled="busy"
          @click="save('txt')"
          >TXT</WlButton
        >
        <WlButton
          size="sm"
          severity="secondary"
          :disabled="busy || !selected?.hasTimestamps"
          @click="save('srt')"
          >SRT</WlButton
        >
        <WlButton
          size="sm"
          severity="secondary"
          :disabled="busy || !selected?.hasTimestamps"
          @click="save('vtt')"
          >VTT</WlButton
        >
      </template>
    </WlDialog>
  </div>
</template>
<style scoped>
.files-drop {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--fono-space-3);
  padding: var(--fono-space-8) var(--fono-space-4);
  border: 1px dashed var(--fono-border);
  border-radius: var(--fono-radius);
  color: var(--fono-secondary);
  text-align: center;
}
.files-drop.hovering {
  border-color: var(--fono-accent);
  background: var(--fono-accent-soft);
}
.files-drop strong {
  color: var(--fono-text);
}
.files-hint {
  margin-block: var(--fono-space-4) var(--fono-space-6);
}
.files-list {
  list-style: none;
  padding: 0;
  margin: 0;
}
.files-row {
  display: flex;
  align-items: center;
  gap: var(--fono-space-3);
  padding: var(--fono-space-4) 0;
  border-bottom: 1px solid var(--fono-border);
}
.file-content {
  display: flex;
  flex: 1;
  min-width: 0;
  flex-direction: column;
  gap: var(--fono-space-1);
}
.file-content strong {
  overflow-wrap: anywhere;
}
progress {
  width: 100%;
  accent-color: var(--fono-accent);
}
.file-error {
  color: var(--fono-error);
  overflow-wrap: anywhere;
}
.file-result {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.files-empty {
  padding-block: var(--fono-space-6);
}
</style>
