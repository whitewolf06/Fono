<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import {
  computed,
  ref,
  watch,
  onScopeDispose,
  defineAsyncComponent,
} from "vue";
import { RouterLink, onBeforeRouteLeave } from "vue-router";
import { WlButton, WlTextarea } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { triggerLabel } from "../../../shared/domain/triggers";
import { useFeedback } from "../../../shared/application/feedback";
import { useInteraction } from "../../../shared/application/interaction";
import { protectUnload } from "../../../shared/infrastructure/browser";
import type { LastSession } from "../../../shared/domain/contracts";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import { liveIsActive } from "../domain/live";
const liveCompiled =
  typeof __FONO_EXPERIMENTAL_LIVE__ !== "undefined" &&
  __FONO_EXPERIMENTAL_LIVE__;
const LiveTextPreview =
  typeof __FONO_EXPERIMENTAL_LIVE__ !== "undefined" &&
  __FONO_EXPERIMENTAL_LIVE__
    ? defineAsyncComponent(() => import("./LiveTextPreview.vue"))
    : null;
const workspace = useWorkspace();
const { run, busy, error } = useFeedback();
const ui = useInteraction();
const editing = ref(false);
const editor = ref("");
const last = computed(() => workspace.state.last);
const live = computed(() => workspace.state.live);
const active = computed(() => liveIsActive(live.value));
const editorDirty = computed(
  () => editing.value && editor.value !== last.value.draft,
);
watch(
  () => last.value.entry?.id,
  () => {
    editing.value = false;
    editor.value = "";
  },
);
async function leave() {
  return (
    !editorDirty.value ||
    ui.confirm({
      get title() {
        return uiMessage("ui.882d45ed17");
      },
      get text() {
        return uiMessage("ui.8df09c770e");
      },
      get accept() {
        return uiMessage("ui.878560af3d");
      },
      danger: true,
    })
  );
}
onBeforeRouteLeave(leave);
onScopeDispose(protectUnload(() => editorDirty.value));
async function choose(variant: LastSession["variant"]) {
  if (
    last.value.edited &&
    !(await ui.confirm({
      get title() {
        return uiMessage("ui.243bff7c0f");
      },
      get text() {
        return uiMessage("ui.6569616ab0");
      },
      get accept() {
        return uiMessage("ui.88d64ef585");
      },
    }))
  )
    return;
  workspace.dictation.chooseVariant(variant);
}
async function cancelEdit() {
  if (await leave()) editing.value = false;
}
function beginEdit() {
  if (active.value) return;
  editor.value = last.value.draft;
  editing.value = true;
}
</script>
<template>
  <section class="text-preview" aria-labelledby="latest-heading">
    <header class="text-preview-header">
      <div class="title-with-icon">
        <span class="text-icon"><AppIcon name="note" :size="20" /></span>
        <div>
          <h2 id="latest-heading">{{ $t("ui.7506d8b903") }}</h2>
          <small>{{
            active
              ? $t("ui.4bc14d90d2")
              : last.entry
                ? last.edited
                  ? $t("ui.fada287b2e")
                  : $t("ui.960f50dd60")
                : $t("ui.0730d4dad2")
          }}</small>
        </div>
      </div>
      <RouterLink class="text-link" to="/history"
        ><AppIcon name="clock" :size="16" />{{
          $t("ui.1417b7792d")
        }}</RouterLink
      >
    </header>
    <LiveTextPreview v-if="liveCompiled && active && live" :live="live" />
    <template v-else-if="last.entry">
      <div class="text-meta">
        <div class="segmented small" :aria-label="$t('ui.0eb55289a4')">
          <button
            v-if="last.entry.original"
            type="button"
            :class="{ selected: last.variant === 'original' }"
            :disabled="editing || busy"
            @click="choose('original')"
          >
            {{ $t("ui.421252fcb5") }}</button
          ><button
            type="button"
            :class="{ selected: last.variant === 'result' }"
            :disabled="editing || busy"
            @click="choose('result')"
          >
            {{
              last.entry.original && last.entry.text !== last.entry.original
                ? $t("ui.c7a53e1149")
                : $t("ui.0e99d4a3d9")
            }}
          </button>
        </div>
        <small v-if="last.edited">{{ $t("ui.4a215c6978") }}</small>
      </div>
      <WlTextarea
        v-if="editing"
        v-model="editor"
        class="latest-editor"
        :aria-label="$t('ui.36baf24f7f')"
        :rows="4"
      />
      <div
        v-else
        class="latest-text"
        tabindex="0"
        :aria-label="$t('ui.7506d8b903')"
      >
        {{ last.draft || $t("ui.6195ffbcea") }}
      </div>
      <footer class="text-actions">
        <template v-if="editing"
          ><WlButton
            size="sm"
            variant="primary"
            @click="
              workspace.dictation.edit(editor);
              editing = false;
            "
            >{{ $t("ui.d317c0a03c") }}</WlButton
          ><WlButton size="sm" @click="cancelEdit">{{
            $t("ui.8fbe9b75cb")
          }}</WlButton></template
        ><template v-else>
          <WlButton
            size="sm"
            variant="ghost"
            :disabled="busy"
            @click="beginEdit"
            ><template #icon><AppIcon name="edit" :size="16" /></template
            >{{ $t("ui.59792556fd") }}</WlButton
          >
          <WlButton
            size="sm"
            variant="soft"
            :loading="busy"
            :disabled="
              !workspace.state.preferences.processingEnabled ||
              workspace.state.preferences.dictationMode === 'live' ||
              !last.draft.trim()
            "
            :title="
              workspace.state.preferences.dictationMode === 'live'
                ? $t('ui.2b2f362c6a')
                : workspace.state.preferences.processingEnabled
                  ? $t('ui.002c83085e')
                  : $t('ui.c0f6aa3b3a')
            "
            @click="
              run(
                () => workspace.dictation.improve(),
                workspace.native ? $t('ui.5d61838ebb') : $t('ui.f02425527d'),
              )
            "
            ><template #icon><AppIcon name="sparkle" :size="16" /></template
            >{{ $t("ui.89a537789d") }}</WlButton
          >
          <WlButton
            v-if="last.undo !== null"
            size="sm"
            variant="ghost"
            @click="workspace.dictation.undoImprove()"
            ><template #icon><AppIcon name="undo" :size="16" /></template
            >{{ $t("ui.82d998ae2a") }}</WlButton
          >
          <WlButton
            size="sm"
            class="push-right"
            :disabled="!last.draft"
            @click="run(() => workspace.copy(last.draft), $t('ui.ec655e25c5'))"
            ><template #icon><AppIcon name="copy" :size="16" /></template
            >{{ $t("ui.1ecddf40bf") }}</WlButton
          >
        </template>
      </footer>
    </template>
    <div v-else class="latest-empty">
      {{ $t("ui.ae1c1a2296") }} {{ triggerLabel(workspace.state.preferences) }}.
    </div>
    <div
      v-if="!active && live?.pendingText && live.insertionState === 'failed'"
      class="text-actions"
    >
      <small class="muted">{{ $t("ui.adce4fa0da") }}</small>
      <WlButton
        size="sm"
        variant="ghost"
        @click="
          run(() => workspace.copy(live!.pendingText), $t('ui.4e5dc8f2b3'))
        "
        >{{ $t("ui.02626effa3") }}</WlButton
      >
    </div>
    <p v-if="error" role="alert" class="error-text">{{ error }}</p>
  </section>
</template>
