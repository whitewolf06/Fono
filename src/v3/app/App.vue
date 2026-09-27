<script setup lang="ts">
import { ref } from "vue";
import VoicePage from "../features/dictation/presentation/VoicePage.vue";
import FonoWordmark from "../shared/FonoWordmark.vue";

type Section = "home" | "history" | "wake" | "model";

const section = ref<Section>("home");
const version = __FONO_FRONTEND_BUILD__.version;
</script>

<template>
  <div class="v3-shell" data-wl-theme="graphite">
    <aside class="v3-sidebar">
      <button
        class="v3-brand"
        type="button"
        aria-label="Fono — главная"
        @click="section = 'home'"
      >
        <span class="v3-brand-mark"><i class="pi pi-wave-pulse"></i></span>
        <span class="v3-brand-copy">
          <FonoWordmark class="v3-brand-wordmark" />
          <small
            ><span class="v3-version-word">Версия </span>{{ version }}</small
          >
        </span>
      </button>

      <nav class="v3-side-nav" aria-label="Основная навигация">
        <button
          class="v3-nav-item"
          aria-label="Главная"
          title="Главная"
          :class="{ 'is-active': section === 'home' }"
          :aria-current="section === 'home' ? 'page' : undefined"
          type="button"
          @click="section = 'home'"
        >
          <i class="pi pi-home"></i><span>Главная</span>
        </button>
        <button
          class="v3-nav-item"
          aria-label="История"
          title="История"
          :class="{ 'is-active': section === 'history' }"
          :aria-current="section === 'history' ? 'page' : undefined"
          type="button"
          @click="section = 'history'"
        >
          <i class="pi pi-history"></i><span>История</span>
        </button>
        <button
          class="v3-nav-item"
          aria-label="Пробуждение"
          title="Пробуждение"
          :class="{ 'is-active': section === 'wake' }"
          :aria-current="section === 'wake' ? 'page' : undefined"
          type="button"
          @click="section = 'wake'"
        >
          <i class="pi pi-microphone"></i><span>Пробуждение</span>
        </button>
        <button
          class="v3-nav-item"
          aria-label="Модель"
          title="Модель"
          :class="{ 'is-active': section === 'model' }"
          :aria-current="section === 'model' ? 'page' : undefined"
          type="button"
          @click="section = 'model'"
        >
          <i class="pi pi-sliders-h"></i><span>Модель</span>
        </button>
        <a
          class="v3-nav-item"
          href="index.html?ui=v2"
          aria-label="Настройки"
          title="Настройки"
        >
          <i class="pi pi-cog"></i><span>Настройки</span>
        </a>
      </nav>
    </aside>

    <div class="v3-main-column">
      <main><VoicePage :section="section" /></main>
    </div>
  </div>
</template>
