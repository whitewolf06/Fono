import type { SettingsDraft } from "../application/useSettingsDraft";

export function WakePhraseInput({
  ariaLabel,
  disabled,
  draft,
  onChange,
}: {
  ariaLabel?: string;
  disabled: boolean;
  draft: SettingsDraft;
  onChange: (value: string) => void;
}) {
  if (draft.supportsCustomWakePhrase) {
    return (
      <input
        aria-label={ariaLabel}
        value={draft.wakePhrase}
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
      />
    );
  }

  const options = wakePhraseOptions(draft);

  return (
    <select
      aria-label={ariaLabel}
      value={draft.wakePhrase}
      disabled={disabled}
      onChange={(event) => onChange(event.target.value)}
    >
      {options.map((phrase) => (
        <option key={phrase} value={phrase}>
          {phrase === draft.wakePhrase && !draft.wakePhraseIsSupported
            ? `${phrase} — не поддерживается`
            : phrase}
        </option>
      ))}
    </select>
  );
}

function wakePhraseOptions(draft: SettingsDraft) {
  const isCurrentListed = draft.wakePhraseOptions.some(
    (phrase) => phrase.toLowerCase() === draft.wakePhrase.toLowerCase(),
  );

  return isCurrentListed
    ? draft.wakePhraseOptions
    : [draft.wakePhrase, ...draft.wakePhraseOptions];
}
