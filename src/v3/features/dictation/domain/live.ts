import type { LiveDictation, Phase } from "../../../shared/domain/contracts";

export function liveIsActive(live: LiveDictation | null | undefined): boolean {
  return !!live && ["listening", "draining"].includes(live.phase);
}

export function livePhase(live: LiveDictation): Phase {
  return live.phase === "draining" ? "transcribing" : live.phase;
}

export function liveStatus(live: LiveDictation): string {
  if (live.phase === "draining") return "Завершаю оставшийся текст";
  if (live.phase === "done") return "Живая диктовка завершена";
  if (live.phase === "cancelled") return "Диктовка отменена · текст сохранён";
  if (live.phase === "error") return "Диктовка прервана";
  if (live.insertionState === "paused_focus") return "Вставка приостановлена";
  if (live.insertionState === "failed") return "Не удалось вставить текст";
  if (live.lagMs > 4000) return "Распознавание догоняет речь";
  if (live.insertionState === "none") return "Слушаю · текст появится в Fono";
  return "Слушаю и вставляю текст";
}

export function assertDraftAvailable(live: LiveDictation | null | undefined) {
  if (liveIsActive(live))
    throw new Error("Сначала завершите диктовку, чтобы изменить текст.");
}
