<script setup lang="ts">
import { computed } from "vue";
import { WlEmpty, WlPill, WlSpinner } from "@whitelife-core/ui-kit";
import { phaseLabels } from "../domain/voice";
import { useVoiceWorkspace } from "../application/useVoiceWorkspace";
import { createVoiceRuntime } from "../infrastructure/voiceRuntime";
import HistoryPanel from "./HistoryPanel.vue";
import ActivityRail from "./ActivityRail.vue";

const workspace = useVoiceWorkspace(createVoiceRuntime());
const phaseLabel = computed(() => phaseLabels[workspace.phase.value]);
</script>

<template>
  <div id="v3-home" class="v3-workspace">
    <WlSpinner v-if="workspace.loading.value" class="v3-loading" />

    <template v-else-if="workspace.overview.value">
      <div class="v3-dashboard">
        <div class="v3-primary">
          <section
            class="v3-hero"
            :class="'is-' + workspace.phase.value"
            aria-label="Диктовка"
          >
            <div class="v3-hero-copy">
              <span class="v3-hero-kicker"
                ><i class="pi pi-bolt"></i> Быстрая транскрибация</span
              >
              <h1>Ваш голос<br /><em>становится текстом.</em></h1>
              <p>
                {{
                  workspace.isListening.value
                    ? "Говорите естественно. Нажмите ещё раз, когда закончите."
                    : "Начните запись кнопкой или горячей клавишей. Текст появится в активном поле."
                }}
              </p>
              <div class="v3-hero-facts">
                <span><i class="pi pi-bolt"></i> Быстро</span>
                <span><i class="pi pi-shield"></i> Локально</span>
                <span><i class="pi pi-desktop"></i> В любом приложении</span>
              </div>
              <span class="v3-hotkey"
                ><kbd>{{ workspace.overview.value.hotkey }}</kbd> горячая
                клавиша</span
              >
            </div>
            <div class="v3-voice-stage">
              <button
                class="v3-voice-orb"
                type="button"
                :aria-label="
                  workspace.isListening.value
                    ? 'Завершить запись'
                    : 'Начать диктовку'
                "
                :disabled="workspace.isProcessing.value || workspace.busy.value"
                @click="
                  workspace.isListening.value
                    ? workspace.stop()
                    : workspace.start()
                "
              >
                <span class="v3-orbit v3-orbit--outer"></span>
                <span class="v3-orbit v3-orbit--middle"></span>
                <span class="v3-orbit v3-orbit--inner"></span>
                <i
                  :class="
                    workspace.isListening.value
                      ? 'pi pi-stop'
                      : 'pi pi-microphone'
                  "
                ></i>
              </button>
              <span class="v3-status"
                ><span class="v3-status-dot"></span>{{ phaseLabel }}</span
              >
            </div>
          </section>

          <p v-if="workspace.message.value" class="v3-message" role="status">
            {{ workspace.message.value }}
          </p>
          <WlPill v-if="workspace.demo" class="v3-demo-pill" variant="info"
            >Демо интерфейса</WlPill
          >

          <div class="v3-feature-grid">
            <section
              id="v3-wake"
              class="v3-panel v3-feature-card"
              aria-labelledby="v3-wake-title"
            >
              <div class="v3-card-heading">
                <span class="v3-card-icon"
                  ><i class="pi pi-microphone"></i
                ></span>
                <div>
                  <h2 id="v3-wake-title">Пробуждение</h2>
                  <p>Запуск голосовой командой</p>
                </div>
              </div>
              <div class="v3-setting-row">
                <span>Ключевая фраза</span
                ><strong>{{
                  workspace.overview.value.wakeWord || "Не задана"
                }}</strong>
              </div>
              <div class="v3-card-bottom">
                <span
                  class="v3-state"
                  :class="{
                    'is-off': !workspace.overview.value.wakeWordEnabled,
                  }"
                >
                  <span class="v3-status-dot"></span
                  >{{
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
            </section>

            <section
              id="v3-model"
              class="v3-panel v3-feature-card"
              aria-labelledby="v3-model-title"
            >
              <div class="v3-card-heading">
                <span class="v3-card-icon"><i class="pi pi-sparkles"></i></span>
                <div>
                  <h2 id="v3-model-title">Распознавание</h2>
                  <p>Текущая модель и язык</p>
                </div>
              </div>
              <div class="v3-setting-row">
                <span>Модель</span
                ><strong :title="workspace.overview.value.model">{{
                  workspace.overview.value.model
                }}</strong>
              </div>
              <div class="v3-setting-row">
                <span>Язык</span
                ><strong>{{ workspace.overview.value.language }}</strong>
              </div>
              <a class="v3-card-link" href="index.html?ui=v2"
                >Все настройки <i class="pi pi-arrow-right"></i
              ></a>
            </section>

            <section
              class="v3-panel v3-feature-card"
              aria-labelledby="v3-hotkey-title"
            >
              <div class="v3-card-heading">
                <span class="v3-card-icon"><i class="pi pi-bolt"></i></span>
                <div>
                  <h2 id="v3-hotkey-title">Быстрый доступ</h2>
                  <p>Работает поверх других окон</p>
                </div>
              </div>
              <div class="v3-setting-row">
                <span>Горячая клавиша</span
                ><kbd>{{ workspace.overview.value.hotkey }}</kbd>
              </div>
              <p class="v3-feature-note">
                Поставьте курсор в нужное поле и начните диктовку.
              </p>
            </section>
          </div>

          <HistoryPanel
            :history="workspace.overview.value.history"
            @refresh="workspace.refresh()"
            @copy="workspace.copyText"
          />
        </div>

        <ActivityRail
          :history="workspace.overview.value.history"
          :hotkey="workspace.overview.value.hotkey"
          :version="workspace.overview.value.version"
        />
      </div>
    </template>
    <WlEmpty
      v-else
      title="Не удалось загрузить Fono"
      :description="workspace.message.value"
    />
  </div>
</template>
