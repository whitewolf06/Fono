<script setup lang="ts">
import { WlButton } from "@whitelife-core/ui-kit";
import type { Scenario } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import { LIVE_DICTATION_ENABLED } from "../../../shared/domain/dictationMode";
import {
  WAKE_WORD_AVAILABLE,
  WAKE_WORD_UNAVAILABLE,
} from "../../../shared/domain/wakeAvailability";
const workspace = useWorkspace();
const scenarios: { id: Scenario; title: string; detail: string }[] = [
  {
    id: "live-paused",
    title: "Живая диктовка · поле изменилось",
    detail: "Запись продолжается, вставка ждёт явного продолжения.",
  },
  {
    id: "live-backlog",
    title: "Живая диктовка · отставание",
    detail: "Аудио сохраняется, распознавание догоняет речь.",
  },
  {
    id: "live-insertion-error",
    title: "Живая диктовка · ошибка вставки",
    detail: "Остаток доступен для копирования; автоматического повтора нет.",
  },
  {
    id: "loading",
    title: "Загрузка данных",
    detail:
      "Экран ожидания на всех страницах. Сценарий зафиксирован для проверки.",
  },
  {
    id: "load-error",
    title: "Ошибка загрузки",
    detail: "Экран ошибки раздела с работающим повтором загрузки.",
  },
  {
    id: "normal",
    title: "Готов к работе",
    detail: "Устройства, история и подключение доступны.",
  },
  {
    id: "empty",
    title: "Пустая история",
    detail: "Нет истории, задач и приложений. Можно начать диктовку.",
  },
  {
    id: "no-microphone",
    title: "Микрофон недоступен",
    detail: "Конкретное действие в карточке и ошибка начала записи.",
  },
  {
    id: "no-model",
    title: "Модель не установлена",
    detail: "Выбор пуст. Загрузите модель в настройках.",
  },
  {
    id: "download",
    title: "Загрузка модели",
    detail: "Прогресс зафиксирован на 46%. Доступна отмена.",
  },
  {
    id: "ai-error",
    title: "ИИ недоступен",
    detail: "Проверка и улучшение возвращают ошибку. Исходный текст остаётся.",
  },
  {
    id: "save-error",
    title: "Ошибка сохранения",
    detail: "Выключатели возвращают прежнее значение. Поля сохраняют правки.",
  },
  {
    id: "service-off",
    title: "Сервис выключен",
    detail: "Приём задач остановлен, результаты доступны.",
  },
  {
    id: "queue",
    title: "Очередь заполнена",
    detail:
      "4 задачи: одна в работе, три в очереди. Выполнение приостановлено для проверки.",
  },
  {
    id: "long-content",
    title: "Длинные значения",
    detail: "Большой текст и длинное название модели.",
  },
];
</script>
<template>
  <div class="page">
    <PageHeading
      title="Проверка интерфейса"
      description="Воспроизводимые демонстрационные состояния V3."
      ><RouterLink class="text-link" to="/"
        >Открыть главную</RouterLink
      ></PageHeading
    >
    <p class="notice">
      Выбор сценария заменяет текущие демонстрационные данные и черновик. Для
      обычного просмотра выберите «Готов к работе». Ни один сценарий не
      обращается к настоящему бэкенду.
    </p>
    <p v-if="!WAKE_WORD_AVAILABLE" class="notice">
      Пробуждение голосом: {{ WAKE_WORD_UNAVAILABLE }}. Это ограничение
      действует во всех сценариях.
    </p>
    <div class="scenario-grid">
      <button
        v-for="scenario in scenarios.filter(
          (item) => LIVE_DICTATION_ENABLED || !item.id.startsWith('live-'),
        )"
        :key="scenario.id"
        class="scenario-card"
        :class="{ selected: workspace.state.scenario === scenario.id }"
        :aria-pressed="workspace.state.scenario === scenario.id"
        @click="workspace.scenario(scenario.id)"
      >
        <div class="section-header">
          <h3>{{ scenario.title }}</h3>
          <AppIcon
            :name="workspace.state.scenario === scenario.id ? 'check' : 'grid'"
          />
        </div>
        <p>{{ scenario.detail }}</p>
      </button>
    </div>
    <div class="surface-panel section-header">
      <div>
        <h2>Отдельные сценарии</h2>
        <p>Первый запуск и состояния плавающего индикатора.</p>
      </div>
      <div class="actions">
        <RouterLink class="text-link" to="/onboarding">Первый запуск</RouterLink
        ><RouterLink class="text-link" to="/overlay">Индикатор</RouterLink
        ><WlButton size="sm" @click="workspace.scenario('normal')"
          >Сбросить демо</WlButton
        >
      </div>
    </div>
  </div>
</template>
