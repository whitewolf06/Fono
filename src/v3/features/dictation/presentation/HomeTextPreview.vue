<script setup lang="ts">
import { computed, ref, watch, onScopeDispose } from "vue";
import { RouterLink, onBeforeRouteLeave } from "vue-router";
import { WlButton, WlTextarea } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import { useInteraction } from "../../../shared/application/interaction";
import { protectUnload } from "../../../shared/infrastructure/browser";
import type { LastSession } from "../../../shared/domain/contracts";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
const workspace = useWorkspace();
const { run, busy, error } = useFeedback();
const ui = useInteraction();
const editing = ref(false);
const editor = ref("");
const last = computed(() => workspace.state.last);
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
      title: "Закрыть редактор?",
      text: "Правки текста ещё не применены. Они будут отменены.",
      accept: "Отменить правки",
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
      title: "Открыть другую версию?",
      text: "Правки текущего черновика будут заменены выбранной версией. Архивная запись останется прежней.",
      accept: "Открыть версию",
    }))
  )
    return;
  workspace.dictation.chooseVariant(variant);
}
async function cancelEdit() {
  if (await leave()) editing.value = false;
}
function beginEdit() {
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
          <h2 id="latest-heading">Последний текст</h2>
          <small>{{
            last.entry
              ? last.edited
                ? "Ваш черновик · архив не изменён"
                : "Последняя диктовка этой сессии"
              : "Здесь появится ваша следующая мысль"
          }}</small>
        </div>
      </div>
      <RouterLink class="text-link" to="/history"
        ><AppIcon name="clock" :size="16" />История</RouterLink
      >
    </header>
    <template v-if="last.entry">
      <div class="text-meta">
        <div class="segmented small" aria-label="Версия текста">
          <button
            v-if="last.entry.original"
            type="button"
            :class="{ selected: last.variant === 'original' }"
            :disabled="editing || busy"
            @click="choose('original')"
          >
            Исходная расшифровка</button
          ><button
            type="button"
            :class="{ selected: last.variant === 'result' }"
            :disabled="editing || busy"
            @click="choose('result')"
          >
            {{
              last.entry.original && last.entry.text !== last.entry.original
                ? "Обработанный текст"
                : "Результат диктовки"
            }}
          </button>
        </div>
        <small v-if="last.edited">Изменён</small>
      </div>
      <WlTextarea
        v-if="editing"
        v-model="editor"
        class="latest-editor"
        aria-label="Редактор последнего текста"
        :rows="4"
      />
      <div v-else class="latest-text" tabindex="0" aria-label="Последний текст">
        {{
          last.draft ||
          "Текст пуст. Нажмите «Редактировать», чтобы добавить его."
        }}
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
            >Применить правки</WlButton
          ><WlButton size="sm" @click="cancelEdit">Отмена</WlButton></template
        ><template v-else>
          <WlButton
            size="sm"
            variant="ghost"
            :disabled="busy"
            @click="beginEdit"
            ><template #icon><AppIcon name="edit" :size="16" /></template
            >Редактировать</WlButton
          >
          <WlButton
            size="sm"
            variant="soft"
            :loading="busy"
            :disabled="
              !workspace.state.preferences.processingEnabled ||
              !last.draft.trim()
            "
            :title="
              workspace.state.preferences.processingEnabled
                ? 'Улучшить текущий черновик'
                : 'Включите обработку текста'
            "
            @click="
              run(
                () => workspace.dictation.improve(),
                workspace.native
                  ? 'Черновик обновлён'
                  : 'Черновик обновлён · демонстрация',
              )
            "
            ><template #icon><AppIcon name="sparkle" :size="16" /></template
            >Улучшить текст</WlButton
          >
          <WlButton
            v-if="last.undo !== null"
            size="sm"
            variant="ghost"
            @click="workspace.dictation.undoImprove()"
            ><template #icon><AppIcon name="undo" :size="16" /></template
            >Отменить улучшение</WlButton
          >
          <WlButton
            size="sm"
            class="push-right"
            :disabled="!last.draft"
            @click="run(() => workspace.copy(last.draft), 'Текст скопирован')"
            ><template #icon><AppIcon name="copy" :size="16" /></template
            >Копировать</WlButton
          >
        </template>
      </footer>
    </template>
    <div v-else class="latest-empty">
      Нажмите «Начать запись» или используйте
      {{ workspace.state.preferences.hotkey }}.
    </div>
    <p v-if="error" role="alert" class="error-text">{{ error }}</p>
  </section>
</template>
