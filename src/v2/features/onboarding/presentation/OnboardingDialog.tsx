import { useState } from "react";
import { createPortal } from "react-dom";

const steps = [
  {
    eyebrow: "Добро пожаловать",
    title: "Fono готов к спокойной работе с голосом",
    description:
      "Диктуйте текст в любое приложение, не отправляя запись в облако.",
  },
  {
    eyebrow: "1 из 3 · Микрофон",
    title: "Проверьте источник записи",
    description:
      "Верхняя карточка «Микрофон» открывает только нужную настройку — устройство и уровень входа.",
  },
  {
    eyebrow: "2 из 3 · Активация",
    title: "Запускайте диктовку удобным способом",
    description:
      "Нажмите большую кнопку или используйте Ctrl + Space. Wake word можно включить в соседней карточке.",
  },
  {
    eyebrow: "3 из 3 · Готово",
    title: "Текст появится там, где стоит курсор",
    description:
      "После диктовки проверьте результат, при необходимости отредактируйте его или откройте историю.",
  },
];

export function OnboardingDialog({ onComplete }: { onComplete: () => void }) {
  const [stepIndex, setStepIndex] = useState(0);
  const step = steps[stepIndex];
  const isLastStep = stepIndex === steps.length - 1;

  const proceed = () => {
    if (isLastStep) {
      onComplete();
      return;
    }
    setStepIndex((index) => index + 1);
  };

  return createPortal(
    <div className="v2-onboarding-backdrop" role="presentation">
      <section
        className="v2-onboarding-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="v2-onboarding-title"
      >
        <div className="v2-onboarding-dialog__progress" aria-hidden="true">
          {steps.map((_, index) => (
            <i key={index} className={index <= stepIndex ? "is-active" : ""} />
          ))}
        </div>
        <span>{step.eyebrow}</span>
        <h2 id="v2-onboarding-title">{step.title}</h2>
        <p>{step.description}</p>
        <footer>
          <button
            className="v2-onboarding-dialog__skip"
            type="button"
            onClick={onComplete}
          >
            Пропустить
          </button>
          <button
            className="v2-onboarding-dialog__next"
            type="button"
            onClick={proceed}
          >
            {isLastStep ? "Начать работу" : "Далее"}
          </button>
        </footer>
      </section>
    </div>,
    document.body,
  );
}
