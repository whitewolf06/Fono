import { useId, useState, type CSSProperties, type ReactNode } from "react";
import type { SettingsStatusDetail } from "../application/useSettingsDraft";

interface SettingsCardProps {
  title: string;
  description: string;
  icon: SettingsIconName;
  focused?: boolean;
  collapsed?: boolean;
  onToggleCollapsed?: () => void;
  children: ReactNode;
}

export type SettingsIconName =
  "general" | "audio" | "activation" | "processing" | "overlay" | "advanced";

export function SettingsCard({
  title,
  description,
  icon,
  focused = false,
  collapsed,
  onToggleCollapsed,
  children,
}: SettingsCardProps) {
  const [uncontrolledCollapsed, setUncontrolledCollapsed] = useState(false);
  const contentId = useId();
  const isCollapsed = collapsed ?? uncontrolledCollapsed;

  const toggleCollapsed = () => {
    if (onToggleCollapsed) {
      onToggleCollapsed();
      return;
    }

    setUncontrolledCollapsed((current) => !current);
  };

  return (
    <section
      className={`v2-settings-card ${focused ? "is-focused" : ""} ${
        isCollapsed ? "is-collapsed" : ""
      }`}
      data-settings-section={icon}
    >
      <header className="v2-settings-card__header">
        <button
          className="v2-settings-card__toggle"
          type="button"
          aria-controls={contentId}
          aria-expanded={!isCollapsed}
          aria-label={`${isCollapsed ? "Развернуть" : "Свернуть"} раздел «${title}»`}
          onClick={toggleCollapsed}
        />
        <span className="v2-settings-card__icon">
          <SectionIcon type={icon} />
        </span>
        <div>
          <h2>{title}</h2>
          <p>{description}</p>
        </div>
        <span className="v2-settings-card__chevron" aria-hidden="true" />
      </header>
      {!isCollapsed && (
        <div className="v2-settings-card__body" id={contentId}>
          {children}
        </div>
      )}
    </section>
  );
}

export function SettingRow({
  title,
  description,
  disabled = false,
  children,
}: {
  title: string;
  description: string;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <div className={`v2-kit-setting ${disabled ? "is-disabled" : ""}`}>
      <div>
        <strong>{title}</strong>
        <span>{description}</span>
      </div>
      {children}
    </div>
  );
}

export function Switch({
  checked,
  onChange,
  disabled = false,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <button
      className={`v2-switch ${checked ? "is-on" : ""}`}
      type="button"
      disabled={disabled}
      aria-pressed={checked}
      onClick={() => onChange(!checked)}
    >
      <i />
    </button>
  );
}

export function RangeField({
  label,
  value,
  onChange,
  min = 0,
  max = 100,
  step = 1,
  suffix,
  disabled = false,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  suffix: string;
  disabled?: boolean;
}) {
  return (
    <label className="v2-field v2-field--range">
      <span>
        {label}
        <b>
          {value}
          {suffix}
        </b>
      </span>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        style={
          {
            "--range": `${((value - min) / (max - min)) * 100}%`,
          } as CSSProperties
        }
        onChange={(event) => onChange(Number(event.target.value))}
      />
    </label>
  );
}

export function SignalPreview() {
  return (
    <div className="v2-settings-signal" aria-label="Уровень микрофона">
      <span>Проверка сигнала</span>
      <div>
        <i />
        <i />
        <i />
        <i />
        <i />
        <i />
        <i />
        <i />
      </div>
      <small>Микрофон отвечает, средний уровень 34%.</small>
    </div>
  );
}

export function SettingsStatus({ status }: { status: SettingsStatusDetail }) {
  const labelByState: Record<SettingsStatusDetail["state"], string> = {
    idle: "Не проверено",
    checking: "Проверка…",
    ready: "Готово",
    error: "Нужна проверка",
  };

  return (
    <div className={`v2-settings-status v2-settings-status--${status.state}`}>
      <span>{labelByState[status.state]}</span>
      <p>{status.message}</p>
    </div>
  );
}

export function SectionIcon({ type }: { type: SettingsIconName }) {
  const paths: Record<SettingsIconName, ReactNode> = {
    general: (
      <path d="M12 3v3M12 18v3M4.2 7.2l2.1 2.1M17.7 14.7l2.1 2.1M3 12h3M18 12h3M4.2 16.8l2.1-2.1M17.7 9.3l2.1-2.1M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z" />
    ),
    audio: (
      <>
        <rect x="8" y="3" width="8" height="12" rx="4" />
        <path d="M5.5 11.5a6.5 6.5 0 0 0 13 0M12 18v3M8.5 21h7" />
      </>
    ),
    activation: <path d="M4 12h2M8 8v8M12 5v14M16 8v8M20 12h-2" />,
    processing: (
      <>
        <path d="M4 7h10M18 7h2M10 12h10M4 12h2M4 17h10M18 17h2" />
        <circle cx="16" cy="7" r="2" />
        <circle cx="8" cy="12" r="2" />
        <circle cx="16" cy="17" r="2" />
      </>
    ),
    overlay: (
      <>
        <rect x="4" y="5" width="16" height="14" rx="3" />
        <path d="M8 10h8M8 14h5" />
      </>
    ),
    advanced: (
      <>
        <path d="M19 12a7 7 0 0 0-.1-1l2-1.5-2-3.4-2.4 1a7.2 7.2 0 0 0-1.7-1L14.5 3h-4l-.3 3.1a7.2 7.2 0 0 0-1.7 1l-2.4-1-2 3.4L6.1 11a7 7 0 0 0 0 2l-2 1.5 2 3.4 2.4-1a7.2 7.2 0 0 0 1.7 1l.3 3.1h4l.3-3.1a7.2 7.2 0 0 0 1.7-1l2.4 1 2-3.4-2-1.5c.1-.3.1-.7.1-1Z" />
        <circle cx="12" cy="12" r="3" />
      </>
    ),
  };

  return (
    <svg className="v2-settings-icon" viewBox="0 0 24 24" aria-hidden="true">
      {paths[type]}
    </svg>
  );
}
