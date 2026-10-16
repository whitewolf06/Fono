<script setup lang="ts">
import { WlButton } from "@whitelife-core/ui-kit";
import type { LiveDictation } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
defineProps<{ live: LiveDictation }>();
const workspace = useWorkspace();
const { run } = useFeedback();
</script>
<template>
  <div
    class="latest-text live-transcript"
    tabindex="0"
    aria-label="Живая расшифровка"
  >
    <span>{{ live.committedText }}</span
    ><span class="live-draft">{{ live.draftText }}</span>
    <small v-if="!live.committedText && !live.draftText" class="muted"
      >Говорите — первые фрагменты появятся через несколько секунд.</small
    >
  </div>
  <footer class="text-actions">
    <small class="muted"
      >{{
        live.draftText
          ? "Приглушённый текст ещё уточняется."
          : "Устойчивые фрагменты больше не переписываются."
      }}
      Редактирование — после завершения.</small
    >
    <WlButton
      v-if="live.pendingText && live.insertionState === 'failed'"
      size="sm"
      variant="ghost"
      title="Проверьте последний фрагмент в поле: часть текста могла уже вставиться"
      @click="
        run(
          () => workspace.copy(live!.pendingText),
          'Остаток скопирован. Проверьте последний фрагмент перед вставкой',
        )
      "
      >Копировать остаток</WlButton
    >
    <WlButton
      size="sm"
      class="push-right"
      :disabled="!live.committedText"
      @click="
        run(
          () => workspace.copy(live!.committedText),
          'Подтверждённый текст скопирован',
        )
      "
    >
      <template #icon><AppIcon name="copy" :size="16" /></template>Копировать
    </WlButton>
  </footer>
</template>
