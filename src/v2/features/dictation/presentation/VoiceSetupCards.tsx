import type {
  DictationSettingsSummary,
  ReadinessSnapshot,
} from "@/v2/shared/domain/pipeline";

export type VoiceSetupTarget =
  "microphone" | "wake-word" | "recognition" | "post-processing";

interface VoiceSetupCardsProps {
  readiness: ReadinessSnapshot | null;
  settingsSummary: DictationSettingsSummary | null;
  onConfigure: (target: VoiceSetupTarget) => void;
  onToggleWakeWord: () => void;
}

export function VoiceSetupCards({
  readiness,
  settingsSummary,
  onConfigure,
  onToggleWakeWord,
}: VoiceSetupCardsProps) {
  if (!readiness || !settingsSummary) return null;

  const wakeWordLabel = {
    active: "Активен",
    paused: "На паузе",
    disabled: "Выключен",
  }[readiness.wakeWord];

  return (
    <section className="v2-voice-setup-cards" aria-label="Текущая конфигурация">
      <article
        className={`v2-voice-setup-card ${
          readiness.microphone === "ready" ? "is-ready" : "is-attention"
        }`}
      >
        <CardConfigureButton
          label="Настроить микрофон"
          onClick={() => onConfigure("microphone")}
        />
        <SetupIcon type="microphone" />
        <CardAction
          label="Настроить микрофон"
          icon="settings"
          onClick={() => onConfigure("microphone")}
        />
        <span>Микрофон</span>
        <strong>
          <i />
          {readiness.microphone === "ready" ? "Готов" : "Проверьте"}
        </strong>
      </article>
      <article
        className={`v2-voice-setup-card ${
          readiness.wakeWord === "active" ? "is-ready" : "is-attention"
        } has-multiple-actions`}
      >
        <CardConfigureButton
          label="Настроить wake word"
          onClick={() => onConfigure("wake-word")}
        />
        <SetupIcon type="wake-word" />
        <div className="v2-voice-setup-card__actions">
          <CardAction
            label={
              readiness.wakeWord === "active"
                ? "Выключить wake word"
                : "Включить wake word"
            }
            icon="power"
            active={readiness.wakeWord === "active"}
            onClick={onToggleWakeWord}
          />
          <CardAction
            label="Настроить wake word"
            icon="settings"
            onClick={() => onConfigure("wake-word")}
          />
        </div>
        <span>Wake word</span>
        <strong>
          <i />
          {wakeWordLabel}
        </strong>
      </article>
      <article
        className={`v2-voice-setup-card ${
          readiness.model === "ready" ? "is-ready" : "is-attention"
        }`}
      >
        <CardConfigureButton
          label="Настроить распознавание"
          onClick={() => onConfigure("recognition")}
        />
        <SetupIcon type="recognition" />
        <CardAction
          label="Настроить распознавание"
          icon="settings"
          onClick={() => onConfigure("recognition")}
        />
        <span>Распознавание</span>
        <strong>{settingsSummary.recognitionModel}</strong>
        <small>{settingsSummary.accelerator}</small>
      </article>
      <article className="v2-voice-setup-card">
        <CardConfigureButton
          label="Настроить постобработку"
          onClick={() => onConfigure("post-processing")}
        />
        <SetupIcon type="post-processing" />
        <CardAction
          label="Настроить постобработку"
          icon="settings"
          onClick={() => onConfigure("post-processing")}
        />
        <span>Постобработка</span>
        <strong>{settingsSummary.postProcessing}</strong>
      </article>
    </section>
  );
}

function CardAction({
  label,
  icon,
  active = false,
  onClick,
}: {
  label: string;
  icon: "settings" | "power";
  active?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      className={`v2-voice-setup-card__action ${active ? "is-active" : ""}`}
      type="button"
      aria-label={label}
      title={label}
      onClick={(event) => {
        event.stopPropagation();
        onClick();
      }}
    >
      <ActionIcon type={icon} />
    </button>
  );
}

function CardConfigureButton({
  label,
  onClick,
}: {
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      className="v2-voice-setup-card__configure"
      type="button"
      aria-label={label}
      onClick={onClick}
    />
  );
}

function SetupIcon({
  type,
}: {
  type: "microphone" | "wake-word" | "recognition" | "post-processing";
}) {
  if (type === "microphone") {
    return (
      <svg
        className="v2-voice-setup-card__icon"
        viewBox="0 0 24 24"
        aria-hidden="true"
      >
        <rect x="8" y="3" width="8" height="12" rx="4" />
        <path d="M5.5 11.5a6.5 6.5 0 0 0 13 0M12 18v3M8.5 21h7" />
      </svg>
    );
  }

  if (type === "wake-word") {
    return (
      <svg
        className="v2-voice-setup-card__icon"
        viewBox="0 0 24 24"
        aria-hidden="true"
      >
        <path d="M4 12h2M8 8v8M12 5v14M16 8v8M20 12h-2" />
      </svg>
    );
  }

  if (type === "recognition") {
    return (
      <svg
        className="v2-voice-setup-card__icon"
        viewBox="0 0 24 24"
        aria-hidden="true"
      >
        <rect x="4" y="5" width="16" height="14" rx="3" />
        <path d="M8 10h1M15 10h1M9 15h6" />
      </svg>
    );
  }

  return (
    <svg
      className="v2-voice-setup-card__icon"
      viewBox="0 0 24 24"
      aria-hidden="true"
    >
      <path d="M4 7h10M18 7h2M10 12h10M4 12h2M4 17h10M18 17h2" />
      <circle cx="16" cy="7" r="2" />
      <circle cx="8" cy="12" r="2" />
      <circle cx="16" cy="17" r="2" />
    </svg>
  );
}

function ActionIcon({ type }: { type: "settings" | "power" }) {
  if (type === "power") {
    return (
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M12 3v9M7.1 5.9a8 8 0 1 0 9.8 0" />
      </svg>
    );
  }

  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="12" cy="12" r="3" />
      <path d="M19 12a7 7 0 0 0-.1-1l2-1.5-2-3.4-2.4 1a7.2 7.2 0 0 0-1.7-1L14.5 3h-4l-.3 3.1a7.2 7.2 0 0 0-1.7 1l-2.4-1-2 3.4L6.1 11a7 7 0 0 0 0 2l-2 1.5 2 3.4 2.4-1a7.2 7.2 0 0 0 1.7 1l.3 3.1h4l.3-3.1a7.2 7.2 0 0 0 1.7-1l2.4 1 2-3.4-2-1.5c.1-.3.1-.7.1-1Z" />
    </svg>
  );
}
