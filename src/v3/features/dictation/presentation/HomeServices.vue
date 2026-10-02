<script setup lang="ts">
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
  <section class="home-services" aria-label="Сервисы">
    <article class="service-card">
      <div class="service-icon"><AppIcon name="server" :size="25" /></div>
      <div class="service-copy">
        <div class="service-heading">
          <RouterLink to="/api/tasks"
            >API-сервис <AppIcon name="chevron-right" :size="14" /></RouterLink
          ><PreferenceToggle name="serviceEnabled" label="API-сервис" compact />
        </div>
        <p>Голосовой ввод для ваших приложений.</p>
        <div class="service-meta">
          <StatusDot
            :health="state.preferences.serviceEnabled ? 'ready' : 'off'"
            :label="state.preferences.serviceEnabled ? 'Включён' : 'Выключен'"
          /><span>{{
            state.preferences.serviceEnabled
              ? (state.serviceAddress || "127.0.0.1:17832") +
                " · в очереди: " +
                queue
              : "Включите для подключения"
          }}</span>
        </div>
      </div>
    </article>
    <article class="service-card">
      <div class="service-icon"><AppIcon name="activity" :size="27" /></div>
      <div class="service-copy">
        <div class="service-heading">
          <RouterLink to="/trainer"
            >Речевой тренер
            <AppIcon name="chevron-right" :size="14" /></RouterLink
          ><PreferenceToggle
            name="trainerEnabled"
            label="Речевой тренер"
            compact
          />
        </div>
        <p>Замечайте привычки. Говорите яснее.</p>
        <div class="service-meta">
          <StatusDot
            :health="state.preferences.trainerEnabled ? 'ready' : 'off'"
            :label="state.preferences.trainerEnabled ? 'Включён' : 'Выключен'"
          /><span>{{
            state.preferences.trainerEnabled
              ? state.history.filter((e) => e.original).length +
                " диктовок для разбора"
              : "Локальный анализ речи"
          }}</span>
        </div>
      </div>
    </article>
  </section>
</template>
