<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../shared/domain/messages";

import { computed, watch } from "vue";
import { RouterLink } from "vue-router";
import { useWorkspace } from "../../shared/application/workspace";
import { useFeedback } from "../../shared/application/feedback";
import { updateBusy } from "../../shared/domain/updates";
import { PROJECT_SITE_URL } from "../../shared/domain/project";
import AppIcon from "../../shared/presentation/AppIcon.vue";

const workspace = useWorkspace();
const { run } = useFeedback();
const { run: checkUpdate, busy: checking, error: checkError } = useFeedback();
const status = workspace.updates.state;
watch(
  () => status.phase,
  (phase) => {
    if (phase !== "error") checkError.value = "";
  },
);
const available = computed(
  () => status.phase === "available" && !!status.nextVersion,
);
const failed = computed(() => status.phase === "error" || !!checkError.value);
const busy = computed(() => updateBusy(status.phase) || checking.value);
const version = computed(
  () => `v${status.nextVersion?.replace(/^v/i, "") ?? ""}`,
);
const label = computed(() => {
  if (available.value)
    return uiMessage("ui.188876fa32", { v0: String(version.value) });
  if (failed.value) return uiMessage("ui.4e395375de");
  switch (status.phase) {
    case "checking":
      return uiMessage("ui.6d03347d77");
    case "downloading":
      return uiMessage("ui.f75e927d03");
    case "installing":
      return uiMessage("ui.6857926fd9");
    case "up_to_date":
      return uiMessage("ui.07200fa94e");
    case "not_configured":
      return uiMessage("ui.dcfc3857f3");
    default:
      return uiMessage("ui.c8d5c5ee6e");
  }
});
const detailTitle = computed(
  () =>
    `${label.value}. ${available.value ? uiMessage("ui.e4673ff7e9") : uiMessage("ui.a3985e950b")}`,
);
</script>
<template>
  <section class="sidebar-extras" :aria-label="$t('ui.4a3e5ba2eb')">
    <nav class="sidebar-extra-nav" :aria-label="$t('ui.238258a075')">
      <RouterLink
        to="/releases"
        class="sidebar-extra-item"
        active-class="active"
        :title="$t('ui.d9e4291457')"
        :aria-label="$t('ui.d9e4291457')"
      >
        <AppIcon name="note" :size="16" /><span class="sidebar-extra-label">{{
          $t("ui.d9e4291457")
        }}</span>
      </RouterLink>
      <RouterLink
        to="/updates"
        class="sidebar-extra-item"
        active-class="active"
        :title="$t('ui.455471db0c')"
        :aria-label="$t('ui.455471db0c')"
      >
        <AppIcon name="download" :size="16" /><span
          class="sidebar-extra-label"
          >{{ $t("ui.455471db0c") }}</span
        >
      </RouterLink>
      <a
        :href="PROJECT_SITE_URL"
        class="sidebar-extra-item"
        :title="$t('ui.0d683a3715')"
        :aria-label="$t('ui.0d683a3715')"
        target="_blank"
        rel="noopener noreferrer"
        @click.prevent="run(() => workspace.openProjectSite())"
      >
        <AppIcon name="globe" :size="16" /><span class="sidebar-extra-label">{{
          $t("ui.c24e9ae442")
        }}</span>
        <AppIcon
          class="sidebar-external-icon"
          name="external-link"
          :size="14"
        />
      </a>
      <RouterLink
        to="/settings/diagnostics?field=feedback"
        active-class="active"
        class="sidebar-extra-item"
        :title="$t('ui.0077ef5ee6')"
        :aria-label="$t('ui.0077ef5ee6')"
      >
        <AppIcon name="message" :size="16" /><span
          class="sidebar-extra-label"
          >{{ $t("ui.930f9a65fb") }}</span
        >
      </RouterLink>
    </nav>
    <div
      class="sidebar-update-row"
      :data-state="failed ? 'error' : available ? 'available' : status.phase"
    >
      <RouterLink
        v-if="available || failed"
        class="sidebar-update-summary"
        to="/updates"
        :title="detailTitle"
        :aria-label="detailTitle"
      >
        <span class="sidebar-update-dot" aria-hidden="true" />
        <span class="sidebar-update-label" role="status">{{ label }}</span>
        <span class="sidebar-update-action">{{
          available ? $t("ui.603e460bf5") : $t("ui.59589fe2a5")
        }}</span>
      </RouterLink>
      <span v-else class="sidebar-update-summary" :title="label">
        <span class="sidebar-update-dot" aria-hidden="true" />
        <span class="sidebar-update-label" role="status">{{ label }}</span>
      </span>
      <button
        type="button"
        class="sidebar-update-check"
        :class="{ checking: status.phase === 'checking' }"
        :disabled="busy || status.phase === 'not_configured'"
        :aria-busy="status.phase === 'checking'"
        :aria-label="$t('ui.93b08f7b14')"
        :title="
          status.phase === 'not_configured'
            ? $t('ui.dcfc3857f3')
            : $t('ui.93b08f7b14')
        "
        @click="checkUpdate(() => workspace.updates.check())"
      >
        <svg
          width="16"
          height="16"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path
            d="M20 7a9 9 0 00-15-2L2 8 M2 3v5h5 M4 17a9 9 0 0015 2l3-3 M22 21v-5h-5"
          />
        </svg>
      </button>
    </div>
  </section>
</template>
