<script setup lang="ts">
import VoicePage from "../features/dictation/presentation/VoicePage.vue";
</script>

<template>
  <div class="v3-shell" data-wl-theme="graphite">
    <aside class="v3-sidebar">
      <a class="v3-brand" href="#v3-home" aria-label="Fono — главная">
        <span class="v3-brand-mark"><i class="pi pi-wave-pulse"></i></span>
        <span><strong>fono</strong><small>Ваш голос. Ваш текст.</small></span>
      </a>
      <nav class="v3-side-nav" aria-label="Основная навигация">
        <a class="v3-nav-item is-active" href="#v3-home" aria-current="page"
          ><i class="pi pi-home"></i><span>Главная</span></a
        >
        <a class="v3-nav-item" href="#v3-history-title"
          ><i class="pi pi-history"></i><span>История</span></a
        >
        <a class="v3-nav-item" href="#v3-wake"
          ><i class="pi pi-microphone"></i><span>Пробуждение</span></a
        >
        <a class="v3-nav-item" href="#v3-model"
          ><i class="pi pi-sliders-h"></i><span>Модель</span></a
        >
        <a class="v3-nav-item" href="index.html?ui=v2"
          ><i class="pi pi-cog"></i><span>Настройки</span></a
        >
      </nav>
      <div class="v3-sidebar-footer">
        <p>Говорите.<br />Мы позаботимся<br />о тексте.</p>
        <span>Fono · интерфейс V3</span>
      </div>
    </aside>
    <div class="v3-main-column">
      <header class="v3-topbar">
        <nav class="v3-top-nav" aria-label="Быстрый переход">
          <a class="is-active" href="#v3-home"
            ><i class="pi pi-home"></i>Главная</a
          >
          <a href="#v3-history-title"><i class="pi pi-history"></i>История</a>
          <a href="#v3-wake"><i class="pi pi-microphone"></i>Пробуждение</a>
          <a href="#v3-model"><i class="pi pi-sliders-h"></i>Модель</a>
          <a href="index.html?ui=v2"><i class="pi pi-cog"></i>Настройки</a>
        </nav>
        <span class="v3-topbar-label"
          ><span class="v3-local-dot"></span>Локально и приватно</span
        >
      </header>
      <main><VoicePage /></main>
    </div>
  </div>
</template>
