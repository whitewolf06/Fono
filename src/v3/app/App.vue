<script setup lang="ts">
import VoicePage from "../features/dictation/presentation/VoicePage.vue";
</script>

<template>
  <div class="v3-shell">
    <aside class="v3-sidebar">
      <div class="v3-brand">
        <span class="v3-brand-mark"><i class="pi pi-microphone"></i></span>
        <span><strong>fono</strong><small>Голосовое пространство</small></span>
      </div>
      <div class="v3-sidebar-label">Рабочая область</div>
      <nav aria-label="Навигация V3">
        <a class="v3-nav-item is-active" href="v3.html" aria-current="page"
          ><i class="pi pi-wave-pulse"></i> Диктовка</a
        >
        <a class="v3-nav-item" href="#v3-history-title"
          ><i class="pi pi-clock"></i> История</a
        >
      </nav>
      <div class="v3-sidebar-bottom">
        <div class="v3-privacy-card">
          <i class="pi pi-shield"></i>
          <strong>Приватность внутри</strong>
          <p>Распознавание выполняется на вашем компьютере.</p>
        </div>
        <a class="v3-legacy-link" href="index.html?ui=v2"
          ><i class="pi pi-cog"></i> Все настройки
          <i class="pi pi-arrow-up-right"></i
        ></a>
      </div>
    </aside>
    <div class="v3-main-column">
      <header class="v3-topbar">
        <div class="v3-breadcrumb">
          <span>Fono</span><i class="pi pi-angle-right"></i
          ><strong>Диктовка</strong>
        </div>
        <div class="v3-topbar-right">
          <span class="v3-local-dot"></span> Локально и приватно
        </div>
      </header>
      <main><VoicePage /></main>
    </div>
  </div>
</template>
