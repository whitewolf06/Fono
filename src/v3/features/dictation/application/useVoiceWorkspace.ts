import { computed, onMounted, onUnmounted, ref } from "vue";
import type { VoiceOverview, VoicePhase } from "../domain/voice";
import type { VoiceRuntime } from "./voiceRuntime";

export function useVoiceWorkspace(runtime: VoiceRuntime) {
  const overview = ref<VoiceOverview | null>(null);
  const loading = ref(true);
  const busy = ref(false);
  const message = ref("");
  let dispose: (() => void) | undefined;

  const phase = computed<VoicePhase>(() => overview.value?.phase ?? "idle");
  const isListening = computed(() => phase.value === "listening");
  const isProcessing = computed(() =>
    ["transcribing", "processing", "injecting"].includes(phase.value),
  );

  async function refresh() {
    try {
      overview.value = await runtime.load();
      message.value = "";
    } catch (cause) {
      message.value = cause instanceof Error ? cause.message : String(cause);
    } finally {
      loading.value = false;
    }
  }

  async function run(action: () => Promise<void>) {
    if (busy.value) return;
    busy.value = true;
    message.value = "";
    try {
      await action();
      await refresh();
    } catch (cause) {
      message.value = cause instanceof Error ? cause.message : String(cause);
    } finally {
      busy.value = false;
    }
  }

  async function copyText(text: string) {
    await run(() => runtime.copyText(text));
    if (!message.value) message.value = "Текст скопирован";
  }

  onMounted(() => {
    dispose = runtime.subscribePhase((next) => {
      if (overview.value) overview.value = { ...overview.value, phase: next };
      if (next === "idle") void refresh();
    });
    void refresh();
  });
  onUnmounted(() => dispose?.());

  return {
    overview,
    loading,
    busy,
    message,
    phase,
    isListening,
    isProcessing,
    demo: runtime.demo,
    refresh,
    start: () => run(() => runtime.start()),
    stop: () => run(() => runtime.stop()),
    toggleWakeWord: () => run(() => runtime.toggleWakeWord()),
    copyText,
  };
}
