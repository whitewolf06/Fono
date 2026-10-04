<script setup lang="ts">
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
  if (available.value) return `Доступна ${version.value}`;
  if (failed.value) return "Ошибка проверки";
  switch (status.phase) {
    case "checking":
      return "Проверяю обновления…";
    case "downloading":
      return "Загрузка обновления…";
    case "installing":
      return "Установка обновления…";
    case "up_to_date":
      return "Версия актуальна";
    case "not_configured":
      return "Обновления не настроены";
    default:
      return "Проверка обновлений";
  }
});
const detailTitle = computed(
  () =>
    `${label.value}. ${available.value ? "Перейти к обновлению" : "Открыть подробности"}`,
);
</script>
<template>
  <section class="sidebar-extras" aria-label="О Fono и обновления">
    <nav class="sidebar-extra-nav" aria-label="Дополнительная навигация">
      <RouterLink
        to="/releases"
        class="sidebar-extra-item"
        active-class="active"
        title="История версий"
        aria-label="История версий"
      >
        <AppIcon name="note" :size="16" /><span class="sidebar-extra-label"
          >История версий</span
        >
      </RouterLink>
      <RouterLink
        to="/updates"
        class="sidebar-extra-item"
        active-class="active"
        title="Обновления"
        aria-label="Обновления"
      >
        <AppIcon name="download" :size="16" /><span class="sidebar-extra-label"
          >Обновления</span
        >
      </RouterLink>
      <a
        :href="PROJECT_SITE_URL"
        class="sidebar-extra-item"
        title="Открыть сайт Fono в браузере"
        aria-label="Открыть сайт Fono в браузере"
        target="_blank"
        rel="noopener noreferrer"
        @click.prevent="run(() => workspace.openProjectSite())"
      >
        <AppIcon name="link" :size="16" /><span class="sidebar-extra-label"
          >Сайт Fono</span
        >
        <AppIcon
          class="sidebar-external-icon"
          name="external-link"
          :size="14"
        />
      </a>
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
          available ? "Обновить" : "Подробнее"
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
        aria-label="Проверить обновления"
        :title="
          status.phase === 'not_configured'
            ? 'Обновления не настроены'
            : 'Проверить обновления'
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
