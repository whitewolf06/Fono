interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  ariaLabel?: string;
}

export function Switch({
  checked,
  onChange,
  disabled = false,
  ariaLabel,
}: SwitchProps) {
  return (
    <button
      className={`v2-switch ${checked ? "is-on" : ""}`}
      type="button"
      disabled={disabled}
      aria-label={ariaLabel}
      aria-pressed={checked}
      onClick={() => onChange(!checked)}
    >
      <i />
    </button>
  );
}
