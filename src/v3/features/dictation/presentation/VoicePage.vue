<script setup lang="ts">
import { computed } from "vue";
import { WlEmpty, WlSpinner } from "@whitelife-core/ui-kit";
import { phaseLabels } from "../domain/voice";
import { useVoiceWorkspace } from "../application/useVoiceWorkspace";
import { createVoiceRuntime } from "../infrastructure/voiceRuntime";
import HistoryPanel from "./HistoryPanel.vue";
import VoiceWave from "./VoiceWave.vue";

const props = defineProps<{
  section: "home" | "history" | "wake" | "model";
}>();

const workspace = useVoiceWorkspace(createVoiceRuntime());
const phaseLabel = computed(() => phaseLabels[workspace.phase.value]);
const heroDescription = computed(() => {
  const hotkey = workspace.overview.value?.hotkey ?? "Ctrl + Space";

  if (workspace.isListening.value) {
    return (
      "Говорите свободно. Нажмите " + hotkey + " ещё раз, чтобы закончить."
    );
  }
  if (workspace.isProcessing.value) {
    return "Сохраняем мысль и готовим текст для вставки.";
  }
  return (
    "Нажмите " + hotkey + ", чтобы начать. Текст появится в активном поле."
  );
});
</script>

<template>
  <div
    class="v3-workspace"
    :class="{ 'v3-workspace--home': props.section === 'home' }"
  >
    <WlSpinner v-if="workspace.loading.value" class="v3-loading" />

    <template v-else-if="workspace.overview.value">
      <section
        v-if="props.section === 'home'"
        class="v3-hero"
        :class="'is-' + workspace.phase.value"
        aria-label="Диктовка"
      >
        <div class="v3-hero-copy">
          <span class="v3-hero-kicker"
            ><i class="pi pi-bolt"></i> Голосовой ввод</span
          >
          <h1>Fono</h1>
          <p>{{ heroDescription }}</p>
          <span class="v3-hotkey">
            <kbd>{{ workspace.overview.value.hotkey }}</kbd>
            <span>горячая клавиша</span>
          </span>
        </div>
        <VoiceWave :phase="workspace.phase.value" :label="phaseLabel" />
      </section>

      <section v-else-if="props.section === 'history'" class="v3-detail">
        <div class="v3-detail-heading">
          <span class="v3-hero-kicker">Ваши записи</span>
          <h1>История</h1>
          <p>Последние результаты диктовки сохранены на вашем компьютере.</p>
        </div>
        <HistoryPanel
          :history="workspace.overview.value.history"
          @refresh="workspace.refresh()"
          @copy="workspace.copyText"
        />
      </section>

      <section v-else-if="props.section === 'wake'" class="v3-detail">
        <div class="v3-detail-heading">
          <span class="v3-hero-kicker">Без клавиатуры</span>
          <h1>Пробуждение</h1>
          <p>Включите голосовую фразу для запуска записи.</p>
        </div>
        <div class="v3-panel v3-feature-card" aria-labelledby="v3-wake-title">
          <div class="v3-card-heading">
            <span class="v3-card-icon"><i class="pi pi-microphone"></i></span>
            <div>
              <h2 id="v3-wake-title">Голосовая команда</h2>
              <p>Запуск диктовки по ключевой фразе</p>
            </div>
          </div>
          <div class="v3-setting-row">
            <span>Ключевая фраза</span>
            <strong>{{
              workspace.overview.value.wakeWord || "Не задана"
            }}</strong>
          </div>
          <div class="v3-card-bottom">
            <span
              class="v3-state"
              :class="{ 'is-off': !workspace.overview.value.wakeWordEnabled }"
            >
              <span class="v3-status-dot"></span>
              {{
                workspace.overview.value.wakeWordEnabled
                  ? "Включено"
                  : "Выключено"
              }}
            </span>
            <button
              class="v3-action-button"
              type="button"
              :disabled="workspace.busy.value"
              @click="workspace.toggleWakeWord()"
            >
              {{
                workspace.overview.value.wakeWordEnabled
                  ? "Выключить"
                  : "Включить"
              }}
            </button>
          </div>
        </div>
      </section>

      <section v-else class="v3-detail">
        <div class="v3-detail-heading">
          <span class="v3-hero-kicker">Распознавание речи</span>
          <h1>Модель</h1>
          <p>Текущие параметры распознавания.</p>
        </div>
        <div class="v3-panel v3-feature-card" aria-labelledby="v3-model-title">
          <div class="v3-card-heading">
            <span class="v3-card-icon"><i class="pi pi-sparkles"></i></span>
            <div>
              <h2 id="v3-model-title">Распознавание</h2>
              <p>Текущая модель и язык</p>
            </div>
          </div>
          <div class="v3-setting-row">
            <span>Модель</span>
            <strong :title="workspace.overview.value.model">{{
              workspace.overview.value.model
            }}</strong>
          </div>
          <div class="v3-setting-row">
            <span>Язык</span
            ><strong>{{ workspace.overview.value.language }}</strong>
          </div>
          <a class="v3-card-link" href="index.html?ui=v2">
            Все настройки <i class="pi pi-arrow-right"></i>
          </a>
        </div>
      </section>

      <p v-if="workspace.message.value" class="v3-message" role="status">
        {{ workspace.message.value }}
      </p>
    </template>

    <WlEmpty
      v-else
      title="Не удалось загрузить Fono"
      :description="workspace.message.value"
    />
  </div>
</template>
