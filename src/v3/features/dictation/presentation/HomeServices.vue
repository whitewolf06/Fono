<script setup lang="ts">
import type { VoiceServices } from "../domain/voice";

defineProps<{ services: VoiceServices }>();
</script>

<template>
  <div class="v3-services" aria-label="Дополнительные возможности">
    <section
      class="v3-service-card v3-service-card--api"
      aria-labelledby="v3-api-title"
    >
      <span class="v3-service-icon" aria-hidden="true">
        <svg viewBox="0 0 64 64" fill="none">
          <path d="M18 18h28v28H18z" />
          <path d="M32 9v9M32 46v9M9 32h9M46 32h9" />
          <circle cx="32" cy="32" r="7" />
          <circle cx="32" cy="8" r="2" />
          <circle cx="32" cy="56" r="2" />
          <circle cx="8" cy="32" r="2" />
          <circle cx="56" cy="32" r="2" />
        </svg>
      </span>
      <div class="v3-service-body">
        <div class="v3-service-heading">
          <div>
            <span class="v3-service-eyebrow">Локальное подключение</span>
            <h2 id="v3-api-title">API сервис</h2>
          </div>
          <span
            class="v3-service-status"
            :class="{ 'is-off': !services.api.enabled }"
          >
            <span class="v3-service-status-dot"></span>
            {{ services.api.enabled ? "Включён" : "Выключен" }}
          </span>
        </div>
        <p class="v3-service-description">
          Транскрибация для приложений и ваших сценариев.
        </p>
        <div class="v3-service-meta">
          <span :title="services.api.address">{{ services.api.address }}</span>
          <span v-if="services.api.enabled"
            >В очереди: {{ services.api.queued }}</span
          >
        </div>
      </div>
    </section>

    <section
      class="v3-service-card v3-service-card--trainer"
      aria-labelledby="v3-trainer-title"
    >
      <span class="v3-service-icon" aria-hidden="true">
        <svg viewBox="0 0 64 64" fill="none">
          <path d="M12 16h40v28H29l-10 8v-8h-7V16Z" />
          <path d="M20 31h4l3-7 5 16 4-12 2 3h6" />
          <path
            d="m46 8 1.3 3.7L51 13l-3.7 1.3L46 18l-1.3-3.7L41 13l3.7-1.3L46 8Z"
          />
        </svg>
      </span>
      <div class="v3-service-body">
        <div class="v3-service-heading">
          <div>
            <span class="v3-service-eyebrow">Ваша речь в динамике</span>
            <h2 id="v3-trainer-title">Речевой тренер</h2>
          </div>
          <span
            class="v3-service-status"
            :class="{ 'is-off': !services.trainer.enabled }"
          >
            <span class="v3-service-status-dot"></span>
            {{ services.trainer.enabled ? "Включён" : "Выключен" }}
          </span>
        </div>
        <p class="v3-service-description">
          Замечает речевые привычки и помогает говорить яснее.
        </p>
        <div class="v3-service-meta">
          <span>Диктовок в истории: {{ services.trainer.dictationCount }}</span>
        </div>
      </div>
    </section>
  </div>
</template>
