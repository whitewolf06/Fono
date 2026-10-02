import { ref, onScopeDispose } from "vue";
import type { Phase } from "../../../shared/domain/contracts";
export function useOverlayDemo() {
  const phase = ref<Phase>("listening");
  const seconds = ref(3);
  const level = ref(0.6);
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
    select(value: Phase) {
      phase.value = value;
      ticks = 0;
      seconds.value = 3;
      isSequence = false;
    },
    play() {
      phase.value = "listening";
      ticks = 0;
      isSequence = true;
    },
  };
}
