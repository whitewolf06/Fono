<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

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
    :aria-label="$t('ui.50d3ea8c13')"
  >
    <span>{{ live.committedText }}</span
    ><span class="live-draft">{{ live.draftText }}</span>
    <small v-if="!live.committedText && !live.draftText" class="muted">{{
      $t("ui.8e2b83032d")
    }}</small>
  </div>
  <footer class="text-actions">
    <small class="muted"
      >{{ live.draftText ? $t("ui.ecd15efa8f") : $t("ui.e0e46e228f") }}
      {{ $t("ui.7f1e1b5f4f") }}</small
    >
    <WlButton
      v-if="live.pendingText && live.insertionState === 'failed'"
      size="sm"
      variant="ghost"
      :title="$t('ui.06d28a4f96')"
      @click="run(() => workspace.copy(live!.pendingText), $t('ui.f44a509581'))"
      >{{ $t("ui.02626effa3") }}</WlButton
    >
    <WlButton
      size="sm"
      class="push-right"
      :disabled="!live.committedText"
      @click="
        run(() => workspace.copy(live!.committedText), $t('ui.d452c15187'))
      "
    >
      <template #icon><AppIcon name="copy" :size="16" /></template
      >{{ $t("ui.1ecddf40bf") }}
    </WlButton>
  </footer>
</template>
