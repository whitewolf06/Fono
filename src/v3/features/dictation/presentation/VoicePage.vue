<script setup lang="ts">
import { computed } from "vue";
import {
  WlButton,
  WlCard,
  WlEmpty,
  WlPill,
  WlSpinner,
} from "@whitelife-core/ui-kit";
import { formatHistoryDate, phaseLabels } from "../domain/voice";
import { useVoiceWorkspace } from "../application/useVoiceWorkspace";
import { createVoiceRuntime } from "../infrastructure/voiceRuntime";

const workspace = useVoiceWorkspace(createVoiceRuntime());
const latest = computed(() => workspace.overview.value?.history[0]);
const phaseLabel = computed(() => phaseLabels[workspace.phase.value]);
</script>

<template>
  <div class="v3-workspace">
    <div class="v3-page-heading">
      <div>
        <span class="v3-eyebrow">Голосовой ввод / Рабочее место</span>
        <h1>Говорите. Fono запишет.</h1>
        <p>
          Ваши слова сразу превращаются в текст и остаются на вашем компьютере.
        </p>
      </div>
      <WlPill v-if="workspace.demo" variant="info">Демо интерфейса</WlPill>
    </div>

    <WlSpinner v-if="workspace.loading.value" class="v3-loading" />

    <template v-else-if="workspace.overview.value">
      <section
        class="v3-hero"
        :class="`is-${workspace.phase.value}`"
        aria-label="Диктовка"
      >
        <div class="v3-hero-copy">
          <span class="v3-status"
            ><span class="v3-status-dot"></span>{{ phaseLabel }}</span
          >
          <h2>Мысль появилась?<br /><em>Просто произнесите её.</em></h2>
          <p>
            {{
              workspace.isListening.value
                ? "Говорите естественно. Когда закончите, остановите запись."
                : "Нажмите кнопку ниже или воспользуйтесь горячей клавишей."
            }}
          </p>
          <div class="v3-hero-actions">
            <WlButton
              variant="primary"
              size="lg"
              :disabled="workspace.isProcessing.value || workspace.busy.value"
              @click="
                workspace.isListening.value
                  ? workspace.stop()
                  : workspace.start()
              "
            >
              <i
                :class="
                  workspace.isListening.value
                    ? 'pi pi-stop'
                    : 'pi pi-microphone'
                "
              ></i>
              {{
                workspace.isListening.value
                  ? "Завершить запись"
                  : "Начать диктовку"
              }}
            </WlButton>
            <span class="v3-hotkey"
              ><kbd>{{ workspace.overview.value.hotkey }}</kbd> горячая
              клавиша</span
            >
          </div>
        </div>
        <div class="v3-voice-art" aria-hidden="true">
          <span class="v3-orbit v3-orbit--outer"></span>
          <span class="v3-orbit v3-orbit--middle"></span>
          <span class="v3-orbit v3-orbit--inner"></span>
          <span class="v3-voice-core"><i class="pi pi-microphone"></i></span>
        </div>
      </section>

      <p v-if="workspace.message.value" class="v3-message" role="status">
        {{ workspace.message.value }}
      </p>

      <div class="v3-content-grid">
        <section class="v3-history" aria-labelledby="v3-history-title">
          <div class="v3-section-heading">
            <div>
              <span class="v3-eyebrow">Ваши слова</span>
              <h2 id="v3-history-title">Недавние записи</h2>
            </div>
            <WlButton
              variant="secondary"
              size="sm"
              @click="workspace.refresh()"
            >
              <i class="pi pi-refresh"></i> Обновить
            </WlButton>
          </div>
          <WlEmpty
            v-if="!workspace.overview.value.history.length"
            title="Пока нет записей"
            description="Начните диктовку, и результат появится здесь."
          />
          <div v-else class="v3-history-list">
            <WlCard
              v-for="(entry, index) in workspace.overview.value.history.slice(
                0,
                4,
              )"
              :key="entry.id"
              class="v3-history-item"
            >
              <div class="v3-history-meta">
                <span>{{ index === 0 ? "Последняя запись" : "Диктовка" }}</span>
                <time>{{ formatHistoryDate(entry.createdAt) }}</time>
              </div>
              <p>{{ entry.text }}</p>
              <WlButton
                variant="ghost"
                size="sm"
                @click="workspace.copyText(entry.text)"
              >
                <i class="pi pi-copy"></i> Скопировать
              </WlButton>
            </WlCard>
          </div>
        </section>

        <aside class="v3-side-panel" aria-label="Состояние Fono">
          <WlCard class="v3-config-card">
            <div class="v3-config-top">
              <i class="pi pi-bolt"></i><span>Сейчас настроено</span>
            </div>
            <dl>
              <div>
                <dt>Модель</dt>
                <dd>{{ workspace.overview.value.model }}</dd>
              </div>
              <div>
                <dt>Язык</dt>
                <dd>{{ workspace.overview.value.language }}</dd>
              </div>
              <div>
                <dt>Ключевая фраза</dt>
                <dd>
                  {{
                    workspace.overview.value.wakeWordEnabled
                      ? "Включена"
                      : "Выключена"
                  }}
                </dd>
              </div>
            </dl>
            <WlButton
              variant="secondary"
              size="sm"
              :disabled="workspace.busy.value"
              @click="workspace.toggleWakeWord()"
            >
              {{
                workspace.overview.value.wakeWordEnabled
                  ? "Выключить фразу"
                  : "Включить фразу"
              }}
            </WlButton>
          </WlCard>
          <div class="v3-tip">
            <span class="v3-tip-icon"><i class="pi pi-sparkles"></i></span>
            <div>
              <strong>Голос свободен от окна</strong>
              <p>
                Используйте горячую клавишу в любом приложении. Fono вставит
                распознанный текст туда, где курсор.
              </p>
            </div>
          </div>
          <div v-if="latest" class="v3-privacy-note">
            <i class="pi pi-lock"></i> История хранится локально
          </div>
          <div class="v3-version">
            Fono {{ workspace.overview.value.version }} · интерфейс V3
          </div>
        </aside>
      </div>
    </template>
    <WlEmpty
      v-else
      title="Не удалось загрузить Fono"
      :description="workspace.message.value"
    />
  </div>
</template>
