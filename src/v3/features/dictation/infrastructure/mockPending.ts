import type {
  Dictation,
  WorkspaceState,
} from "../../../shared/domain/contracts";
import type {
  PendingDictationRequest,
  ProcessingPreset,
  TranslationLanguage,
} from "../../../shared/domain/processing";
import { canonicalizeDictionary } from "../../../shared/domain/personalDictionary";
import { processDemoText } from "./mockText";
import {
  publishDemoEntry,
  selectLatest,
  type CaptureSnapshot,
} from "./mockSession";

interface PendingCapture {
  sessionId: number;
  entry: Dictation;
  snapshot: CaptureSnapshot;
  generationStarted: number;
}

export function createPendingWorkflow(
  state: WorkspaceState,
  delay: (milliseconds: number) => Promise<void>,
  onCancel: () => void,
) {
  let revision = 0;
  let capture: PendingCapture | null = null;
  let inFlight: Promise<void> | null = null;
  function clear() {
    revision++;
    capture = null;
    inFlight = null;
    state.pendingDictation = null;
  }
  function enqueue(value: PendingCapture, error: string | null = null) {
    clear();
    capture = value;
    const preferences = value.snapshot.preferences;
    state.pendingDictation = {
      sessionId: value.sessionId,
      phase: "awaiting_action",
      originalText: value.entry.original || value.entry.text,
      resultText: null,
      createdAt: value.entry.createdAt,
      preset: preferences.processingMode,
      targetLanguage:
        preferences.processingTranslation === "none"
          ? null
          : preferences.processingTranslation,
      processingEnabled: preferences.processingEnabled,
      source:
        state.recordingSource === "hotkey"
          ? "hotkey"
          : state.recordingSource === "wake_word"
            ? "wake_word"
            : "ui",
      error,
      insertionBlocked: false,
    };
    selectLatest(state, value.entry);
    state.phase = "awaiting_action";
    state.error = error || "";
  }
  function complete(
    current: PendingCapture,
    text: string,
    preset: ProcessingPreset | "off",
    processingDurationMs: number | null,
  ) {
    const canonical = canonicalizeDictionary(
      current.snapshot.preferences,
      text,
    );
    publishDemoEntry(
      state,
      current.snapshot,
      current.entry,
      canonical,
      preset,
      current.generationStarted,
      processingDurationMs,
    );
    clear();
    state.phase = "done";
    state.error = "";
  }
  async function process(
    current: PendingCapture,
    preset: ProcessingPreset,
    targetLanguage: TranslationLanguage | null,
  ) {
    const ticket = ++revision;
    const started = Date.now();
    const pending = state.pendingDictation!;
    Object.assign(pending, {
      phase: "processing",
      preset,
      targetLanguage,
      error: null,
    });
    state.phase = "processing";
    state.error = "";
    await delay(800);
    if (ticket !== revision || capture !== current) return;
    if (!state.aiAvailable) {
      const error =
        "Демонстрационная модель недоступна. Проверьте подключение, повторите обработку или вставьте исходный текст.";
      pending.phase = "awaiting_action";
      pending.error = error;
      state.phase = "awaiting_action";
      state.error = error;
      return;
    }
    pending.resultText = processDemoText(
      pending.originalText,
      preset,
      targetLanguage,
    );
    complete(current, pending.resultText, preset, Date.now() - started);
  }
  async function resolve(request: PendingDictationRequest): Promise<void> {
    const current = capture;
    const pending = state.pendingDictation;
    if (!current || !pending || current.sessionId !== request.sessionId)
      throw new Error("Эта диктовка уже завершена или отменена.");
    if (request.action === "cancel") {
      onCancel();
      return;
    }
    if (inFlight) {
      if (request.action === "process_and_insert") return inFlight;
      throw new Error("Сначала дождитесь обработки или отмените диктовку.");
    }
    if (request.action === "insert_raw") {
      complete(current, pending.originalText, "off", null);
      return;
    }
    if (!pending.processingEnabled)
      throw new Error("Обработка текста выключена для этой диктовки.");
    const running = process(
      current,
      request.preset || pending.preset,
      request.targetLanguage === undefined
        ? pending.targetLanguage
        : request.targetLanguage,
    );
    inFlight = running;
    try {
      await running;
    } finally {
      if (inFlight === running) inFlight = null;
    }
  }
  return { clear, enqueue, resolve };
}
