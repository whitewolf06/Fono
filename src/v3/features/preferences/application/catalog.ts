import type { Section } from "../../../shared/domain/contracts";
import type { WlIconName } from "@whitelife-core/ui-kit";
export const sections: {
  id: Section;
  title: string;
  icon: WlIconName;
  description: string;
}[] = [
  {
    id: "general",
    title: "Основные",
    icon: "grid",
    description: "Запуск Fono и поведение приложения.",
  },
  {
    id: "audio",
    title: "Аудио и распознавание",
    icon: "microphone",
    description: "Откуда слушать и как превращать голос в текст.",
  },
  {
    id: "activation",
    title: "Голосовая активация",
    icon: "lightning",
    description: "Начало диктовки без лишних действий.",
  },
  {
    id: "processing",
    title: "Обработка текста и ИИ",
    icon: "sparkle",
    description: "Чистый текст с сохранением вашего смысла.",
  },
  {
    id: "overlay",
    title: "Индикатор записи",
    icon: "panel",
    description: "Небольшой индикатор поверх других окон.",
  },
  {
    id: "privacy",
    title: "История и приватность",
    icon: "eye",
    description: "Вы решаете, какие данные сохранять.",
  },
  {
    id: "diagnostics",
    title: "Диагностика",
    icon: "activity",
    description: "Состояние компонентов и понятные действия.",
  },
];
export const searchIndex: {
  label: string;
  section: Section;
  field: string;
  keywords: string;
}[] = [
  {
    label: "Микрофон",
    section: "audio",
    field: "microphone",
    keywords: "звук вход устройство гарнитура сигнал",
  },
  {
    label: "Модель распознавания",
    section: "audio",
    field: "model",
    keywords: "whisper stt скачать загрузка ускорение cuda язык",
  },
  {
    label: "Горячая клавиша",
    section: "activation",
    field: "hotkey",
    keywords: "hotkey shortcut ctrl space сочетание",
  },
  {
    label: "Фраза пробуждения",
    section: "activation",
    field: "wakePhrase",
    keywords: "wakeword wake слово активация",
  },
  {
    label: "Завершение по тишине",
    section: "activation",
    field: "silenceMs",
    keywords: "тишина тишине пауза задержка конец диктовки",
  },
  {
    label: "Калибровка и пороги",
    section: "activation",
    field: "advanced",
    keywords: "чувствительность vad шум порог",
  },
  {
    label: "Обработка текста",
    section: "processing",
    field: "processingMode",
    keywords: "пунктуация очистка форматирование llm постобработка",
  },
  {
    label: "Подключение и модели ИИ",
    section: "processing",
    field: "profile",
    keywords: "openai lmstudio ключ профиль api сервер ии",
  },
  {
    label: "Пользовательская инструкция",
    section: "processing",
    field: "advanced",
    keywords: "prompt промпт подсказка",
  },
  {
    label: "Рекомендации тренера через ИИ",
    section: "processing",
    field: "trainerModel",
    keywords: "тренер анализ метрики облако согласие",
  },
  {
    label: "Вид индикатора",
    section: "overlay",
    field: "overlayScale",
    keywords: "overlay плавающий прозрачность масштаб компактный положение",
  },
  {
    label: "История и срок хранения",
    section: "privacy",
    field: "retentionDays",
    keywords: "удалить приватность записи сохранять",
  },
  {
    label: "Локальная аналитика речи",
    section: "privacy",
    field: "trainerEnabled",
    keywords: "тренер исходный текст согласие",
  },
  {
    label: "Автозапуск",
    section: "general",
    field: "autostart",
    keywords: "старт windows запуск",
  },
  {
    label: "Вставка текста",
    section: "general",
    field: "insertion",
    keywords: "буфер clipboard sendinput ввод",
  },
  {
    label: "Версия и первый запуск",
    section: "general",
    field: "about",
    keywords: "версия мастер обучение настройка",
  },
  {
    label: "Журнал диагностики",
    section: "diagnostics",
    field: "logs",
    keywords: "логи ошибка проблема проверка",
  },
];
