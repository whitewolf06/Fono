<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

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
  get queued() {
    return uiMessage("ui.b22a2454bd");
  },
  get running() {
    return uiMessage("ui.7894eb1ac6");
  },
  get done() {
    return uiMessage("ui.ef05d57959");
  },
  get error() {
    return uiMessage("ui.bf22dfeeba");
  },
  get cancelled() {
    return uiMessage("ui.23a2a9bfcf");
  },
};
function openResult(id: string) {
  error.value = "";
  notice.value = "";
  selectedId.value = id;
}
function report(value: unknown) {
  error.value =
    value instanceof Error ? value.message : uiMessage("ui.174ecd6048");
}
function added(result: AddFilesResult) {
  notice.value = result.accepted.length
    ? uiMessage("ui.2fe320696a", { v0: String(result.accepted.length) })
    : "";
  if (result.rejected)
    error.value = uiMessage("ui.0a2e267a03", { v0: String(result.rejected) });
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
      notice.value = uiMessage("ui.2d40b6f360");
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
      :title="$t('ui.9088ca9c0d')"
      :description="$t('ui.f796c1ad63')"
    >
      <WlButton
        :loading="busy"
        @click="act(async () => added(await workspace.files.choose()))"
        ><AppIcon name="plus" />{{
          workspace.native ? $t("ui.5199a68348") : $t("ui.95a92a3b7b")
        }}</WlButton
      >
    </PageHeading>
    <p v-if="!workspace.native" class="notice">{{ $t("ui.f2dc2ac857") }}</p>
    <div class="files-drop" :class="{ hovering }">
      <AppIcon name="file" :size="28" />
      <strong>{{ $t("ui.5489566f6a") }}</strong>
      <span>{{ $t("ui.ef938d9907") }}</span>
    </div>
    <p class="muted files-hint">{{ $t("ui.2c71272b95") }}</p>
    <p v-if="error" class="notice error" role="alert">{{ error }}</p>
    <p v-if="notice" class="notice" role="status">{{ notice }}</p>
    <div class="section-header">
      <h2>
        {{ $t("ui.b9c10a4431") }} <span class="muted">{{ jobs.length }}</span>
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
        >{{ $t("ui.ec38c8e8f4") }}</WlButton
      >
    </div>
    <p v-if="!jobs.length" class="muted files-empty">
      {{ $t("ui.360fbc7cbb") }}
    </p>
    <ol v-else class="files-list">
      <li v-for="job in jobs" :key="job.id" class="files-row">
        <AppIcon name="file" />
        <div class="file-content">
          <strong>{{ job.name }}</strong
          ><span class="muted"
            >{{ job.preparing ? $t("ui.61b578bca0") : labels[job.state]
            }}<template v-if="job.seconds">
              · {{ Math.floor(job.seconds / 60) }} {{ $t("ui.62dcfe2d4d") }}
              {{ job.seconds % 60 }} {{ $t("ui.a1f5d39a12") }}</template
            ></span
          >
          <progress
            v-if="job.state === 'running' && !job.preparing"
            :value="job.progress || 0"
            max="1"
            :aria-label="$t('ui.4a8d6d4626', { v0: String(job.name) })"
          />
          <p v-if="job.error" class="file-error">{{ job.error }}</p>
        </div>
        <WlButton
          v-if="['queued', 'running'].includes(job.state)"
          size="sm"
          severity="secondary"
          :disabled="busy"
          @click="act(() => workspace.files.cancel(job.id))"
          >{{ $t("ui.7c47f7292b") }}</WlButton
        >
        <WlButton
          v-else-if="job.state === 'done'"
          size="sm"
          severity="secondary"
          @click="openResult(job.id)"
          >{{ $t("ui.4997a5143d") }}</WlButton
        >
      </li>
    </ol>
    <WlDialog
      v-model:visible="visible"
      :header="selected?.name || $t('ui.4997a5143d')"
      width="560px"
    >
      <p class="file-result">{{ selected?.text || $t("ui.59d2915727") }}</p>
      <p v-if="selected && !selected.hasTimestamps" class="muted">
        {{ $t("ui.cc88503243") }}
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
