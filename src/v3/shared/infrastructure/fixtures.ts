import type {
  Dictation,
  SpeechModel,
  ServiceJob,
  LaunchApp,
} from "../domain/contracts";
export function demoHistory(): Dictation[] {
  const now = new Date();
  const date = (days: number, hour: number) => {
    const d = new Date(now);
    d.setDate(d.getDate() - days);
    d.setHours(hour, 24, 0, 0);
    if (d > now) d.setDate(d.getDate() - 1);
    return d.toISOString();
  };
  return [
    {
      id: "dictation-1",
      title: "Идея для команды",
      createdAt: date(0, 14),
      duration: 42,
      original:
        "Так, давайте, ну, соберём все идеи в одном месте. Мне кажется, это поможет нам быстрее договориться и ничего не потерять. Сначала запишем вопросы, потом выберем один понятный шаг. Вот, после встречи каждый сможет вернуться к заметке и спокойно проверить, что мы решили и кто берёт следующую часть работы.",
      text: "Давайте соберём все идеи в одном месте. Это поможет нам быстрее договориться и ничего не потерять.",
    },
    {
      id: "dictation-2",
      title: "Планы на завтра",
      createdAt: date(0, 11),
      duration: 28,
      original:
        "Завтра сначала проверю новый интерфейс, потом, потом отправлю команде короткий список замечаний. Хочу пройти обычный путь от открытия настроек до готовой записи. Если встречу ошибку, сохраню последовательность действий и ожидаемый результат. Так будет проще повторить проблему и понять, помогло ли исправление.",
      text: "Завтра сначала проверю новый интерфейс, потом отправлю команде короткий список замечаний.",
    },
    {
      id: "dictation-3",
      title: "Заметка о продукте",
      createdAt: date(1, 16),
      duration: 65,
      original:
        "Хочется, чтобы настройки были понятными. То есть чтобы важное всегда было под рукой, а остальное не отвлекало. Пользователь должен видеть, какие данные сохраняются и куда уйдёт запрос. Вернее, это нужно объяснить рядом с действием, пока человек выбирает подключение, а не после запуска обработки.",
      text: "Хочется, чтобы важные настройки всегда были под рукой, а остальные не отвлекали.",
    },
    {
      id: "dictation-4",
      title: "Сообщение клиенту",
      createdAt: date(3, 10),
      duration: 34,
      text: "Спасибо за обратную связь. Подготовлю обновлённый вариант и отправлю его в четверг.",
    },
    {
      id: "dictation-5",
      title: "Мысль для встречи",
      createdAt: date(9, 12),
      duration: 53,
      original:
        "Ну, мы можем начать с короткого примера и потом перейти к деталям. В общем, так будет понятнее.",
      text: "Можем начать с короткого примера и потом перейти к деталям. Так будет понятнее.",
    },
  ];
}
export const demoModels = (): SpeechModel[] =>
  [
    {
      id: "tiny",
      name: "Whisper Tiny",
      size: "75 МБ",
      description: "Whisper Tiny · FP16",
      status: "available",
      progress: 0,
    },
    {
      id: "small",
      name: "Whisper Small",
      size: "466 МБ",
      description: "Whisper Small · FP16",
      status: "installed",
      progress: 100,
    },
    {
      id: "medium",
      name: "Whisper Medium",
      size: "1,5 ГБ",
      description: "Whisper Medium · FP16",
      status: "available",
      progress: 0,
    },
    {
      id: "large",
      name: "Whisper Large v3",
      size: "3,1 ГБ",
      description: "Whisper Large v3 · FP16",
      status: "available",
      progress: 0,
    },
  ]
    .map((model): SpeechModel => ({
      ...model,
      status: model.status as SpeechModel["status"],
      engine: "whisper",
      license: "MIT",
      licenseUrl: "https://github.com/openai/whisper/blob/main/LICENSE",
      languages: ["ru", "en"],
      multilingual: true,
      punctuation: true,
      languageSelection: true,
      decodingOptions: true,
      accelerations: ["auto", "cpu", "cuda", "vulkan"],
    }))
    .concat([
      {
        id: "giga_am:v3_e2e_rnnt_int8",
        engine: "giga_am",
        name: "GigaAM v3 · e2e RNNT INT8",
        size: "221 МБ",
        description: "Пакет e2e RNNT · INT8 encoder",
        status: "available",
        progress: 0,
        license: "MIT",
        licenseUrl:
          "https://github.com/salute-developers/GigaAM/blob/main/LICENSE",
        languages: ["ru"],
        multilingual: false,
        punctuation: true,
        languageSelection: false,
        decodingOptions: false,
        accelerations: ["auto", "cpu"],
      },
      {
        id: "parakeet:tdt_0.6b_v3_int8",
        engine: "parakeet",
        name: "Parakeet v3 · TDT 0.6B INT8",
        size: "639 МБ",
        description: "Пакет TDT · INT8",
        status: "available",
        progress: 0,
        license: "CC-BY-4.0",
        licenseUrl: "https://creativecommons.org/licenses/by/4.0/",
        languages: [
          "bg",
          "hr",
          "cs",
          "da",
          "nl",
          "en",
          "et",
          "fi",
          "fr",
          "de",
          "el",
          "hu",
          "it",
          "lv",
          "lt",
          "mt",
          "pl",
          "pt",
          "ro",
          "sk",
          "sl",
          "es",
          "sv",
          "ru",
          "uk",
        ],
        multilingual: false,
        punctuation: true,
        languageSelection: false,
        decodingOptions: false,
        accelerations: ["auto", "cpu"],
      },
    ]);
export const demoJobs = (): ServiceJob[] => [
  {
    id: "job-01",
    name: "meeting-note.wav",
    state: "done",
    text: "Обсудили следующий этап разработки и согласовали сроки.",
    seconds: 32,
  },
  {
    id: "job-02",
    name: "voice-message.ogg",
    state: "error",
    error: "Формат не поддерживается. Отправьте WAV или MP3.",
    seconds: 0,
  },
];
export const demoApps = (): LaunchApp[] => [
  {
    id: "app-1",
    name: "Браузер",
    phrase: "открой браузер",
    path: "C:\\Program Files\\Browser\\browser.exe",
  },
  {
    id: "app-2",
    name: "Блокнот",
    phrase: "открой блокнот",
    path: "notepad.exe",
  },
];
