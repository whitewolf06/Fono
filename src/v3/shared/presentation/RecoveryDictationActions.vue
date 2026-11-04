<script setup lang="ts">
import { WlButton } from "@whitelife-core/ui-kit";
import { message } from "../domain/messages";
import {
  recoveryTitle,
  type RecoveryDictation,
  type RecoveryRequest,
} from "../domain/recovery";
defineProps<{ recovery: RecoveryDictation; showTitle?: boolean }>();
const emit = defineEmits<{ resolve: [request: RecoveryRequest] }>();
</script>
<template>
  <section class="recovery-actions" :aria-label="message('recovery.actions')">
    <strong v-if="showTitle" role="status">{{
      recoveryTitle(recovery)
    }}</strong>
    <p>{{ message("recovery.retained") }}</p>
    <small v-if="!recovery.local_available">{{
      message("recovery.localMissing")
    }}</small>
    <div class="recovery-actions-buttons">
      <WlButton
        size="sm"
        variant="soft"
        :disabled="recovery.phase === 'recognizing'"
        @click="
          emit('resolve', { session_id: recovery.session_id, action: 'retry' })
        "
        >{{ message("recovery.retry") }}</WlButton
      >
      <WlButton
        size="sm"
        variant="soft"
        :disabled="
          recovery.phase === 'recognizing' || !recovery.local_available
        "
        @click="
          emit('resolve', {
            session_id: recovery.session_id,
            action: 'recognize_local',
          })
        "
        >{{ message("recovery.local") }}</WlButton
      >
      <WlButton
        size="sm"
        variant="ghost"
        @click="
          emit('resolve', { session_id: recovery.session_id, action: 'cancel' })
        "
        >{{ message("recovery.cancel") }}</WlButton
      >
    </div>
  </section>
</template>
<style scoped>
.recovery-actions {
  min-width: 0;
  color: var(--fono-secondary);
  font-size: var(--fono-type-sm);
}
.recovery-actions p {
  margin-block: var(--fono-space-2);
  line-height: 1.5;
}
.recovery-actions small {
  display: block;
  color: var(--fono-muted);
  margin-bottom: var(--fono-space-2);
}
.recovery-actions-buttons {
  display: flex;
  flex-wrap: wrap;
  gap: var(--fono-space-2);
}
</style>
