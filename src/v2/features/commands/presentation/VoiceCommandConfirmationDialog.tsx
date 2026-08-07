import { useEffect, useMemo, useState } from "react";
import { createTauriVoiceCommandRuntime } from "../infrastructure/tauriVoiceCommandRuntime";

export function VoiceCommandConfirmationDialog() {
  const runtime = useMemo(createTauriVoiceCommandRuntime, []);
  const [command, setCommand] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    void runtime.getPending().then(setCommand);
    let unlisten: (() => void) | undefined;
    void runtime
      .subscribe((next) => {
        setStatus(null);
        setCommand(next);
      })
      .then((dispose) => {
        unlisten = dispose;
      });
    return () => unlisten?.();
  }, [runtime]);

  if (!command) return null;

  const cancel = async () => {
    await runtime.cancel();
    setCommand(null);
  };
  const confirm = async () => {
    try {
      const result = await runtime.confirm();
      setStatus(result);
      window.setTimeout(() => setCommand(null), 900);
    } catch (cause) {
      setStatus(
        cause instanceof Error ? cause.message : "Команда не выполнена.",
      );
    }
  };

  return (
    <div className="v2-quick-settings-backdrop" role="presentation">
      <section
        className="v2-quick-settings-dialog"
        role="dialog"
        aria-modal="true"
      >
        <header>
          <div>
            <span>Голосовая команда</span>
            <h2>Подтвердить действие?</h2>
            <p>{command}</p>
          </div>
        </header>
        {status && <div className="v2-settings-status is-ready">{status}</div>}
        <footer>
          <button
            className="v2-button"
            type="button"
            onClick={() => void cancel()}
          >
            Отмена
          </button>
          <button
            className="v2-button v2-button--primary"
            type="button"
            onClick={() => void confirm()}
          >
            Выполнить
          </button>
        </footer>
      </section>
    </div>
  );
}
