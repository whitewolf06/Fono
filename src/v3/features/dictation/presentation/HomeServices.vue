<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

import { computed } from "vue";
import { RouterLink } from "vue-router";
import { useWorkspace } from "../../../shared/application/workspace";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import { PreferenceToggle } from "../../preferences";
import StatusDot from "../../../shared/presentation/StatusDot.vue";
const { state } = useWorkspace();
const queue = computed(
  () =>
    state.jobs.filter((job) => ["queued", "running"].includes(job.state))
      .length,
);
</script>
<template>
  <section class="home-services" :aria-label="$t('ui.621f2f6b4d')">
    <article class="service-card">
      <div class="service-icon"><AppIcon name="server" :size="25" /></div>
      <div class="service-copy">
        <div class="service-heading">
          <RouterLink to="/api/tasks"
            >{{ $t("ui.1b580ef9f9") }}
            <AppIcon name="chevron-right" :size="14" /></RouterLink
          ><PreferenceToggle
            name="serviceEnabled"
            :label="$t('ui.1b580ef9f9')"
            compact
          />
        </div>
        <p>{{ $t("ui.8ad2c0e877") }}</p>
        <div class="service-meta">
          <StatusDot
            :health="state.preferences.serviceEnabled ? 'ready' : 'off'"
            :label="
              state.preferences.serviceEnabled
                ? $t('ui.0b81629905')
                : $t('ui.5a577d9056')
            "
          /><span>{{
            state.preferences.serviceEnabled
              ? (state.serviceAddress || "127.0.0.1:17832") +
                $t("ui.39253e69df") +
                queue
              : $t("ui.2be708ff29")
          }}</span>
        </div>
      </div>
    </article>
    <article class="service-card">
      <div class="service-icon"><AppIcon name="activity" :size="27" /></div>
      <div class="service-copy">
        <div class="service-heading">
          <RouterLink to="/trainer"
            >{{ $t("ui.387054796e") }}
            <AppIcon name="chevron-right" :size="14" /></RouterLink
          ><PreferenceToggle
            name="trainerEnabled"
            :label="$t('ui.387054796e')"
            compact
          />
        </div>
        <p>{{ $t("ui.7763ace8de") }}</p>
        <div class="service-meta">
          <StatusDot
            :health="state.preferences.trainerEnabled ? 'ready' : 'off'"
            :label="
              state.preferences.trainerEnabled
                ? $t('ui.0b81629905')
                : $t('ui.5a577d9056')
            "
          /><span>{{
            state.preferences.trainerEnabled
              ? state.history.filter((e) => e.original).length +
                $t("ui.1c6773e71c")
              : $t("ui.592761cfb0")
          }}</span>
        </div>
      </div>
    </article>
  </section>
</template>
