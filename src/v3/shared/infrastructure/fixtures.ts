import type {
  Dictation,
  SpeechModel,
  ServiceJob,
  LaunchApp,
} from "../domain/contracts";
export function demoHistory(): Dictation[] {
  const date = (days: number, hour: number) => {
    const d = new Date();
    d.setDate(d.getDate() - days);
    d.setHours(hour, 24, 0, 0);
    return d.toISOString();
  };
  return [
    {
      id: "dictation-1",
      title: "Идея для команды",
      createdAt: date(0, 14),
      duration: 42,
      original:
        "Так, давайте, ну, соберём все идеи в одном месте. Мне кажется, это поможет нам быстрее договориться и ничего не потерять.",
      text: "Давайте соберём все идеи в одном месте. Это поможет нам быстрее договориться и ничего не потерять.",
    },
    {
      id: "dictation-2",
      title: "Планы на завтра",
      createdAt: date(0, 11),
      duration: 28,
      original:
        "Завтра сначала проверю новый интерфейс, потом, потом отправлю команде короткий список замечаний.",
      text: "Завтра сначала проверю новый интерфейс, потом отправлю команде короткий список замечаний.",
    },
    {
      id: "dictation-3",
      title: "Заметка о продукте",
      createdAt: date(1, 16),
      duration: 65,
      original:
        "Хочется, чтобы настройки были понятными. То есть чтобы важное всегда было под рукой, а остальное не отвлекало.",
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
export const demoModels = (): SpeechModel[] => [
  {
    id: "tiny",
    name: "Whisper Tiny",
    size: "75 МБ",
    description: "Быстро, для коротких заметок",
    status: "available",
    progress: 0,
  },
  {
    id: "small",
    name: "Whisper Small",
    size: "466 МБ",
    description: "Баланс скорости и качества",
    status: "installed",
    progress: 100,
  },
  {
    id: "medium",
    name: "Whisper Medium",
    size: "1,5 ГБ",
    description: "Для сложной речи и длинных фраз",
    status: "available",
    progress: 0,
  },
  {
    id: "large",
    name: "Whisper Large v3",
    size: "3,1 ГБ",
    description: "Максимальная точность",
    status: "available",
    progress: 0,
  },
];
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
