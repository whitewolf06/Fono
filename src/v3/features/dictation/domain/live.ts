import { message as uiMessage } from "../../../shared/domain/messages";
import type { LiveDictation, Phase } from "../../../shared/domain/contracts";

export function liveIsActive(live: LiveDictation | null | undefined): boolean {
  return (
    typeof __FONO_EXPERIMENTAL_LIVE__ !== "undefined" &&
    __FONO_EXPERIMENTAL_LIVE__ &&
    !!live &&
    ["listening", "draining"].includes(live.phase)
  );
}

export function livePhase(live: LiveDictation): Phase {
  if (!(
    typeof __FONO_EXPERIMENTAL_LIVE__ !== "undefined" &&
    __FONO_EXPERIMENTAL_LIVE__
  ))
    return "idle";
  return live.phase === "draining" ? "transcribing" : live.phase;
}

export function liveStatus(live: LiveDictation): string {
  if (!(
    typeof __FONO_EXPERIMENTAL_LIVE__ !== "undefined" &&
    __FONO_EXPERIMENTAL_LIVE__
  ))
    return "";
  if (live.phase === "draining") return uiMessage("ui.85c67b4904");
  if (live.phase === "done") return uiMessage("ui.8f24ea6e5d");
  if (live.phase === "cancelled") return uiMessage("ui.9e51bcd85d");
  if (live.phase === "error") return uiMessage("ui.65c1264035");
  if (live.insertionState === "paused_focus") return uiMessage("ui.8739567f47");
  if (live.insertionState === "failed") return uiMessage("ui.dc729b1c62");
  if (live.lagMs > 4000) return uiMessage("ui.29036ed392");
  if (live.insertionState === "none") return uiMessage("ui.7d7f9c201a");
  return uiMessage("ui.e8211b626e");
}

export function assertDraftAvailable(live: LiveDictation | null | undefined) {
  if (liveIsActive(live)) throw new Error(uiMessage("ui.ec61866238"));
}
