import { ref, onScopeDispose } from "vue";
import type { Phase, LiveDictation } from "../../../shared/domain/contracts";
export function useOverlayDemo() {
  const phase = ref<Phase>("listening");
  const seconds = ref(3);
  const level = ref(0.6);
  const live = ref<LiveDictation | null>(null);
  let ticks = 0;
  let isSequence = false;
  const sequence: Phase[] = [
    "listening",
    "silence",
    "transcribing",
    "processing",
    "done",
  ];
  const timer = setInterval(() => {
    level.value = Math.abs(Math.sin(++ticks * 0.4)) * 0.8;
    if (phase.value === "silence")
      seconds.value = Math.max(0, 3 - (Math.floor(ticks / 5) % 4));
    if (isSequence && ticks % 18 === 0) {
      const next = sequence[sequence.indexOf(phase.value) + 1];
      if (next) phase.value = next;
      else isSequence = false;
    }
  }, 200);
  onScopeDispose(() => clearInterval(timer));
  return {
    phase,
    seconds,
    level,
    live,
    selectLive(state: "recording" | "paused" | "backlog" | "error") {
      isSequence = false;
      phase.value = "listening";
      seconds.value = 0;
      live.value = {
        sessionId: "overlay-demo",
        revision: 1,
        committedText: "Подтверждённая мысль уже в поле.",
        draftText: "Следующая фраза уточняется",
        pendingText:
          state === "paused" ? "Подтверждённая мысль уже в поле." : "",
        insertionState:
          state === "paused"
            ? "paused_focus"
            : state === "error"
              ? "failed"
              : "active",
        phase: "listening",
        lagMs: state === "backlog" ? 8500 : 2200,
      };
    },
    resumeLive() {
      if (live.value && live.value.insertionState !== "failed")
        live.value.insertionState = "active";
    },
    finish() {
      phase.value = "transcribing";
      if (live.value) live.value.phase = "draining";
    },
    cancel() {
      phase.value = "cancelled";
      if (live.value) live.value.phase = "cancelled";
    },
    select(value: Phase) {
      live.value = null;
      phase.value = value;
      ticks = 0;
      seconds.value = 3;
      isSequence = false;
    },
    play() {
      live.value = null;
      phase.value = "listening";
      ticks = 0;
      isSequence = true;
    },
  };
}
