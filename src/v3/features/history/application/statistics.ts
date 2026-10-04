import type { Dictation } from "../../../shared/domain/contracts";

export function exactDateLabel(date: string): string {
  return new Date(date).toLocaleString("ru-RU", {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function durationLabel(milliseconds?: number | null): string {
  if (
    milliseconds == null ||
    !Number.isFinite(milliseconds) ||
    milliseconds < 0
  )
    return "Не сохранено";
  if (milliseconds < 1000) return Math.round(milliseconds) + " мс";
  return (
    new Intl.NumberFormat("ru-RU", { maximumFractionDigits: 2 }).format(
      milliseconds / 1000,
    ) + " с"
  );
}

export function recordingLabel(entry: Dictation): string {
  const measured = entry.metadata?.recordingDurationMs;
  if (measured != null) return durationLabel(measured);
  return entry.duration > 0
    ? durationLabel(entry.duration * 1000)
    : "Длительность не сохранена";
}

export function dictationFacts(
  entry: Dictation,
): { title: string; value: string }[] {
  const m = entry.metadata;
  const facts = [
    { title: "Запись", value: recordingLabel(entry) },
    {
      title: "Генерация текста",
      value: durationLabel(m?.generationDurationMs),
    },
    { title: "Распознавание", value: durationLabel(m?.recognitionDurationMs) },
    {
      title: "Фактический backend",
      value: m?.backend
        ? { cpu: "CPU", cuda: "NVIDIA CUDA", vulkan: "Vulkan" }[m.backend]
        : "Не сохранён",
    },
    { title: "Модель", value: m?.model || "Не сохранена" },
  ];
  if (m?.requestedAcceleration)
    facts.push({
      title: "Выбранное ускорение",
      value: {
        auto: "Авто",
        cpu: "CPU",
        cuda: "NVIDIA CUDA",
        vulkan: "Vulkan",
      }[m.requestedAcceleration],
    });
  if (m?.language)
    facts.push({
      title: "Язык",
      value:
        { auto: "Авто", ru: "Русский", en: "Английский" }[m.language] ||
        m.language,
    });
  if (m?.processingMode)
    facts.push({
      title: "Обработка текста (настройка)",
      value: {
        off: "Выключена",
        clean: "Очистка",
        format: "Форматирование",
        command: "Команда",
        task: "Постановка задачи",
        formal: "Деловое письмо",
      }[m.processingMode],
    });
  if (m?.processingTrigger)
    facts.push({
      title: "Запуск обработки",
      value:
        m.processingTrigger === "manual"
          ? "По выбору после записи"
          : "Автоматически",
    });
  if (m?.processingTranslation)
    facts.push({
      title: "Перевод результата",
      value: {
        en: "Английский",
        ru: "Русский",
        de: "Немецкий",
        fr: "Французский",
        es: "Испанский",
      }[m.processingTranslation],
    });
  if (m?.dictionaryEnabled !== undefined)
    facts.push({
      title: "Личный словарь",
      value: m.dictionaryEnabled ? "Включён" : "Выключен",
    });
  return facts;
}

export function dictationStages(
  entry: Dictation,
): { title: string; value: string }[] {
  const m = entry.metadata;
  const stages = [];
  for (const [title, duration] of [
    ["Подготовка модели", m?.modelLoadDurationMs],
    ["Обработка текста", m?.processingDurationMs],
  ] as const) {
    if (duration != null && Number.isFinite(duration) && duration >= 0)
      stages.push({ title, value: durationLabel(duration) });
  }
  if (m?.detectedLanguage)
    stages.push({
      title: "Определённый язык",
      value:
        { ru: "Русский", en: "Английский" }[m.detectedLanguage] ||
        m.detectedLanguage,
    });
  return stages;
}
