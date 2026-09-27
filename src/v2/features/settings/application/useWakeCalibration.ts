import { useEffect, useState } from "react";
import type { WakeCalibrationStatus } from "@/lib/types";

export interface WakeCalibrationStore {
  getStatus(): Promise<WakeCalibrationStatus>;
  start(): Promise<WakeCalibrationStatus>;
  recordNext(): Promise<WakeCalibrationStatus>;
  cancel(): Promise<WakeCalibrationStatus>;
}

export type WakeCalibrationState = "idle" | "checking" | "ready" | "error";

export interface WakeCalibrationController {
  status: WakeCalibrationStatus;
  state: WakeCalibrationState;
  message: string;
  start: () => void;
  recordNext: () => void;
  cancel: () => void;
}

const initialStatus: WakeCalibrationStatus = {
  active: false,
  recording: false,
  required_samples: 10,
  accepted_samples: 0,
  rejected_samples: 0,
  phrase: "рамзи",
  latest_result: null,
  profile: null,
};

export function useWakeCalibration(
  store?: WakeCalibrationStore,
): WakeCalibrationController {
  const [status, setStatus] = useState(initialStatus);
  const [state, setState] = useState<WakeCalibrationState>("idle");
  const [message, setMessage] = useState(
    "Запишите «рамзи» десять раз: аудио не сохраняется.",
  );

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

  const run = (operation: (runtime: WakeCalibrationStore) => Promise<WakeCalibrationStatus>) => {
    if (!store) {
      setState("error");
      setMessage("Калибровка доступна в desktop-приложении Fono.");
      return;
    }
    setState("checking");
    setMessage("Подготавливаю локальную калибровку…");
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
    recordNext: () => run((runtime) => runtime.recordNext()),
    cancel: () => run((runtime) => runtime.cancel()),
  };
}

function statusMessage(status: WakeCalibrationStatus) {
  if (status.recording) return "Идёт запись. Произнесите «рамзи» один раз.";
  if (status.active) {
    const result = status.latest_result;
    if (result?.reason === "silence") return "Слишком тихо. Повторите фразу ближе к микрофону.";
    if (result?.reason === "clipping") return "Перегрузка микрофона. Говорите чуть тише или дальше.";
    if (result?.reason === "too_short") return "Фраза получилась слишком короткой. Повторите «рамзи» отчётливо.";
    if (result?.accepted) return "Образец принят. Можно записывать следующий.";
    return `Готово ${status.accepted_samples}/${status.required_samples}. Произнесите «рамзи» один раз.`;
  }
  if (status.profile?.phrase === "рамзи") {
    return "Калибровка завершена: сохранены только итоговые параметры, без аудио.";
  }
  return "Запишите «рамзи» десять раз: аудио не сохраняется.";
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : "Не удалось выполнить калибровку.";
}
