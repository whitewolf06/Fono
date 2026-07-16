export interface BuiltInCommandGroup {
  id: "system" | "media" | "windows";
  title: string;
  description: string;
  icon: string;
  commands: Array<{
    phrase: string;
    result: string;
  }>;
}

export interface LaunchAppDraft {
  id: string;
  name: string;
  executablePath: string;
  aliases: string;
}

export interface CommandPreview {
  title: string;
  description: string;
  tone: "neutral" | "ready" | "attention";
}

export const builtInCommandGroups: BuiltInCommandGroup[] = [
  {
    id: "system",
    title: "Система",
    description: "Громкость и звук Windows.",
    icon: "◐",
    commands: [
      { phrase: "громче", result: "Увеличить громкость" },
      { phrase: "тише", result: "Уменьшить громкость" },
      { phrase: "выключи звук", result: "Включить или выключить mute" },
    ],
  },
  {
    id: "media",
    title: "Медиа",
    description: "Управление активным плеером.",
    icon: "▶",
    commands: [
      { phrase: "пауза", result: "Play / Pause" },
      { phrase: "следующий трек", result: "Переключить трек вперёд" },
      { phrase: "предыдущий трек", result: "Переключить трек назад" },
    ],
  },
  {
    id: "windows",
    title: "Окна и приложения",
    description: "Работа с открытыми окнами и списком приложений.",
    icon: "▣",
    commands: [
      {
        phrase: "переключись на Telegram",
        result: "Найти и активировать окно",
      },
      { phrase: "открой VS Code", result: "Найти и активировать окно" },
      {
        phrase: "запусти Telegram",
        result: "Запустить из настроенного списка",
      },
    ],
  },
];

export function previewCommand(
  phrase: string,
  volumeStep: number,
  applications: LaunchAppDraft[],
): CommandPreview {
  const normalized = phrase.trim().toLocaleLowerCase();

  if (!normalized) {
    return {
      title: "Введите фразу для проверки",
      description: "Предпросмотр не выполняет действие в Windows.",
      tone: "neutral",
    };
  }

  if (normalized.includes("громче")) {
    return {
      title: `Громкость +${volumeStep}%`,
      description: "Будет отправлена системная media-команда Windows.",
      tone: "ready",
    };
  }

  if (normalized.includes("тише")) {
    return {
      title: `Громкость −${volumeStep}%`,
      description: "Будет отправлена системная media-команда Windows.",
      tone: "ready",
    };
  }

  if (normalized.includes("выключи звук") || normalized.includes("без звука")) {
    return {
      title: "Переключить mute",
      description: "Будет отправлена системная media-команда Windows.",
      tone: "ready",
    };
  }

  if (
    normalized.includes("пауза") ||
    normalized.includes("следующий") ||
    normalized.includes("предыдущий")
  ) {
    return {
      title: "Управление воспроизведением",
      description: "Команда будет передана активному медиаплееру.",
      tone: "ready",
    };
  }

  const application = applications.find((app) => {
    const aliases = app.aliases
      .split(",")
      .map((alias) => alias.trim().toLocaleLowerCase())
      .filter(Boolean);
    const terms = [app.name.toLocaleLowerCase(), ...aliases].filter(Boolean);

    return terms.some((term) => normalized.includes(term));
  });

  if (application && normalized.includes("запусти")) {
    return {
      title: `Запустить «${application.name}»`,
      description: "Fono использует путь и алиасы из карточки приложения.",
      tone: "ready",
    };
  }

  if (
    normalized.includes("переключись на") ||
    normalized.includes("перейди в") ||
    normalized.startsWith("открой")
  ) {
    return {
      title: "Найти открытое окно",
      description:
        "Fono сравнит название команды с заголовками видимых окон Windows.",
      tone: "ready",
    };
  }

  return {
    title: "Фраза пока не распознана как команда",
    description: "Проверьте формулировку или добавьте приложение и его алиасы.",
    tone: "attention",
  };
}
