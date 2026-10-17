<script setup lang="ts">
import { message } from "../domain/messages";
import { WlButton, WlSkeleton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../application/workspace";
import AppIcon from "./AppIcon.vue";
const workspace = useWorkspace();
</script>
<template>
  <div class="page">
    <div
      v-if="workspace.state.scenario === 'loading'"
      class="loading-page"
      role="status"
      aria-live="polite"
    >
      <span class="eyebrow">{{ message("load.loading") }}</span
      ><WlSkeleton width="240px" height="30px" /><WlSkeleton
        width="70%"
        height="16px"
      /><WlSkeleton width="100%" height="150px" /><WlSkeleton
        width="100%"
        height="100px"
      />
      <p class="muted">{{ message("load.preparing") }}</p>
      <RouterLink v-if="!workspace.native" class="text-link" to="/scenarios">{{
        message("load.scenarios")
      }}</RouterLink>
    </div>
    <div v-else class="empty-state" role="alert">
      <AppIcon name="warn" :size="30" />
      <h2>{{ message("load.failed") }}</h2>
      <p v-if="workspace.native">{{ workspace.state.error }}</p>
      <p v-else>{{ message("load.demoError") }}</p>
      <WlButton @click="workspace.scenario('normal')">{{
        message("common.retry")
      }}</WlButton
      ><RouterLink v-if="!workspace.native" class="text-link" to="/scenarios">{{
        message("load.scenarios")
      }}</RouterLink>
    </div>
  </div>
</template>
