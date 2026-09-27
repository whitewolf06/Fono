import { useEffect, useState } from "react";
import type {
  WakeProfileValidationKind,
  WakeProfileValidationStatus,
} from "@/lib/types";

export interface WakeProfileValidationStore {
  getStatus(): Promise<WakeProfileValidationStatus>;
  start(): Promise<WakeProfileValidationStatus>;
  record(kind: WakeProfileValidationKind): Promise<WakeProfileValidationStatus>;
}

export type WakeProfileValidationState =
  | "idle"
  | "checking"
  | "ready"
  | "error";

export interface WakeProfileValidationController {
  status: WakeProfileValidationStatus;
  state: WakeProfileValidationState;
  message: string;
  start: () => void;
  record: (kind: WakeProfileValidationKind) => void;
}

const initialStatus: WakeProfileValidationStatus = {
  active: false,
  recording: false,
  failed: false,
  completed: false,
  positive_passed: 0,
  positive_required: 3,
  silence_passed: false,
  other_phrase_passed: false,
  negative_required: 2,
  latest_result: null,
};

export function useWakeProfileValidation(
  store?: WakeProfileValidationStore,
): WakeProfileValidationController {
  const [status, setStatus] = useState(initialStatus);
  const [state, setState] = useState<WakeProfileValidationState>("idle");
  const [message, setMessage] = useState("Профиль пока не проверен.");

  useEffect(() => {
    if (!store) return;

    let active = true;
    void store.getStatus().then(
      (nextStatus) => {
        if (!active) return;
        setStatus(nextStatus);
        setState("ready");
        setMessage(statusMessage(nextStatus));
      },
      (error: unknown) => {
        if (!active) return;
        setState("error");
        setMessage(errorMessage(error));
      },
    );
    return () => {
      active = false;
    };
  }, [store]);

  const run = (
    operation: (runtime: WakeProfileValidationStore) => Promise<WakeProfileValidationStatus>,
  ) => {
    if (!store) {
      setState("error");
      setMessage("Проверка профиля доступна в desktop-приложении Fono.");
      return;
    }
    setState("checking");
    setMessage("Записываю и проверяю образец локально…");
    void operation(store).then(
      (nextStatus) => {
        setStatus(nextStatus);
        setState("ready");
        setMessage(statusMessage(nextStatus));
      },
      (error: unknown) => {
        setState("error");
        setMessage(errorMessage(error));
      },
    );
  };

  return {
    status,
    state,
    message,
    start: () => run((runtime) => runtime.start()),
    record: (kind) => run((runtime) => runtime.record(kind)),
  };
}

function statusMessage(status: WakeProfileValidationStatus) {
  const result = status.latest_result;
  if (status.recording) return "Идёт проверочная запись.";
  if (result?.input_issue === "silence") {
    return "Речи не слышно. Повторите проверочный образец.";
  }
  if (result?.input_issue === "unexpected_speech") {
    return "В тишину попала речь или заметный шум. Повторите её в тихом месте.";
  }
  if (result?.input_issue === "clipping") {
    return "Перегрузка микрофона. Говорите тише и повторите образец.";
  }
  if (result?.input_issue === "too_short") {
    return "Фраза слишком короткая. Повторите её отчётливо.";
  }
  if (status.failed && result?.kind === "positive") {
    return "Sherpa не распознал «рамзи». Повторите калибровку или используйте hotkey.";
  }
  if (status.failed) {
    return "Обнаружено ложное срабатывание. Wake word не будет включён; используйте hotkey.";
  }
  if (status.active) {
    return `Принято ${status.positive_passed}/${status.positive_required} положительных образца.`;
  }
  return "Профиль готов к проверке.";
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : "Не удалось проверить wake-профиль.";
}
