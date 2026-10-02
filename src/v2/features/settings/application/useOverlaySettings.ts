import { useState } from "react";
import type {
  SettingsDraft,
  SettingsDraftStore,
  SettingsStatusDetail,
} from "./useSettingsDraft";

export function useOverlaySettings(
  draft: SettingsDraft,
  store?: SettingsDraftStore,
) {
  const [overlayStatus, setStatus] = useState<SettingsStatusDetail>({
    state: "idle",
    message: "Тестовый показ ещё не запускался.",
  });

  const run = async (
    action: (() => Promise<void>) | undefined,
    message: string,
  ) => {
    if (!action) {
      setStatus({
        state: "error",
        message: "Показ и сброс оверлея доступны в desktop-приложении Fono.",
      });
      return;
    }
    setStatus({ state: "checking", message: "Обновляю оверлей…" });
    try {
      await action();
      setStatus({ state: "ready", message });
    } catch (error) {
      setStatus({
        state: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    }
  };

  const showOverlayTest = () => {
    if (!draft.overlayVisible) {
      setStatus({
        state: "error",
        message: "Включите оверлей, чтобы показать тестовое окно.",
      });
      return;
    }
    return run(
      store?.showOverlayTest ? () => store.showOverlayTest!(draft) : undefined,
      "Оверлей показан на 5 секунд с выбранными параметрами. Микрофон не включается.",
    );
  };

  const resetOverlayPosition = () =>
    run(
      store?.resetOverlayPosition
        ? () => store.resetOverlayPosition!()
        : undefined,
      "Оверлей возвращён в центр экрана и показан на 5 секунд. Его оформление сохранено.",
    );

  return { overlayStatus, showOverlayTest, resetOverlayPosition };
}
