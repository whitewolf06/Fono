import { useState } from "react";

type Step = "welcome" | "mic" | "model" | "wake" | "llm" | "done";

const STEPS: { id: Step; label: string }[] = [
  { id: "welcome", label: "Привет" },
  { id: "mic", label: "Микрофон" },
  { id: "model", label: "Модель" },
  { id: "wake", label: "Активация" },
  { id: "llm", label: "AI" },
  { id: "done", label: "Готово" },
];

export function OnboardingView() {
  const [step, setStep] = useState<Step>("welcome");
  const idx = STEPS.findIndex((s) => s.id === step);

  const next = () => {
    const n = STEPS[Math.min(idx + 1, STEPS.length - 1)];
    setStep(n.id);
  };
  const prev = () => {
    const p = STEPS[Math.max(idx - 1, 0)];
    setStep(p.id);
  };

  return (
    <div className="mx-auto flex min-h-screen max-w-2xl flex-col px-6 py-10">
      <header className="mb-8">
        <h1 className="text-3xl font-semibold tracking-tight">
          Добро пожаловать в Fono
        </h1>
        <p className="mt-2 text-neutral-400">
          Голосовой ввод для Windows — настроим за минуту.
        </p>
      </header>

      {/* Stepper */}
      <ol className="mb-8 flex items-center gap-2">
        {STEPS.map((s, i) => (
          <li key={s.id} className="flex items-center gap-2">
            <span
              className={`flex h-7 w-7 items-center justify-center rounded-full text-xs font-medium ${
                i < idx
                  ? "bg-brand-500 text-white"
                  : i === idx
                    ? "bg-brand-500 text-white ring-4 ring-brand-500/20"
                    : "bg-neutral-800 text-neutral-400"
              }`}
            >
              {i + 1}
            </span>
            <span className="hidden text-sm text-neutral-300 sm:inline">
              {s.label}
            </span>
            {i < STEPS.length - 1 && (
              <span
                className={`h-px w-6 ${i < idx ? "bg-brand-500" : "bg-neutral-700"}`}
              />
            )}
          </li>
        ))}
      </ol>

      <main className="card flex-1">
        {step === "welcome" && (
          <div>
            <h2 className="mb-3 text-xl">Привет! 👋</h2>
            <p className="text-neutral-300">
              Fono превращает вашу речь в текст в любом окне Windows.
              Всё работает локально — аудио и текст не покидают ваш компьютер.
            </p>
            <p className="mt-3 text-neutral-300">
              Можно активировать голосовой ввод ключевой фразой (как «Hey Siri»)
              или горячей клавишей. Давайте настроим.
            </p>
          </div>
        )}
        {step === "mic" && (
          <div>
            <h2 className="mb-3 text-xl">Выберите микрофон</h2>
            <p className="text-neutral-300">
              Мы будем слушать с устройства по умолчанию. Если хотите другое —
              поменяйте в настройках.
            </p>
            <div className="mt-4 rounded-lg bg-neutral-800 p-4 text-sm text-neutral-300">
              🎙️ Системный микрофон по умолчанию
            </div>
          </div>
        )}
        {step === "model" && (
          <div>
            <h2 className="mb-3 text-xl">Модель распознавания</h2>
            <p className="text-neutral-300">
              Мы используем whisper.cpp — лучший локальный движок. Рекомендуем
              начать с модели <code className="text-brand-300">base</code> (140 МБ,
              быстро) или <code className="text-brand-300">small</code> (460 МБ,
              точнее).
            </p>
            <p className="mt-3 text-neutral-400">
              Скачивание моделей будет доступно в разделе «Модель» настроек.
            </p>
          </div>
        )}
        {step === "wake" && (
          <div>
            <h2 className="mb-3 text-xl">Активация</h2>
            <p className="text-neutral-300">
              <strong>Push-to-talk</strong> (по умолчанию): зажмите{" "}
              <kbd className="rounded bg-neutral-700 px-2 py-0.5">Ctrl</kbd>{" "}
              <kbd className="rounded bg-neutral-700 px-2 py-0.5">Space</kbd> и
              диктуйте.
            </p>
            <p className="mt-3 text-neutral-300">
              <strong>Ключевая фраза</strong> («Эй, ассистент»): приложение
              постоянно слушает и активируется на фразу. Будет доступно в версии
              0.2.
            </p>
          </div>
        )}
        {step === "llm" && (
          <div>
            <h2 className="mb-3 text-xl">AI-обработка (опционально)</h2>
            <p className="text-neutral-300">
              Если у вас запущен <strong>LM Studio</strong> с локальным сервером
              на <code className="text-brand-300">localhost:1234</code>, мы
              будем чистить и форматировать ваши слова через LLM.
            </p>
            <p className="mt-3 text-neutral-400">
              Это опционально — без LLM вы получите «сырой» транскрипт.
            </p>
          </div>
        )}
        {step === "done" && (
          <div>
            <h2 className="mb-3 text-xl">Готово! 🎉</h2>
            <p className="text-neutral-300">
              Зажмите <kbd className="rounded bg-neutral-700 px-2 py-0.5">Ctrl</kbd>{" "}
              <kbd className="rounded bg-neutral-700 px-2 py-0.5">Space</kbd> и
              скажите что-нибудь — текст должен появиться в активном окне.
            </p>
            <p className="mt-3 text-neutral-400">
              Настройки всегда доступны из иконки в трее.
            </p>
          </div>
        )}
      </main>

      <footer className="mt-6 flex justify-between">
        <button className="btn-ghost" onClick={prev} disabled={idx === 0}>
          ← Назад
        </button>
        {step !== "done" ? (
          <button className="btn-primary" onClick={next}>
            Далее →
          </button>
        ) : (
          <button
            className="btn-primary"
            onClick={() => window.close()}
          >
            Начать пользоваться
          </button>
        )}
      </footer>
    </div>
  );
}
