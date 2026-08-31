import { DEFAULT_SETTINGS } from "@/lib/types";
import type {
  DictationHistoryEntry,
  Settings,
  SpeechPeriodReport,
} from "@/lib/types";
import type { SpeechTrainerStore } from "../application/useSpeechTrainer";

const now = new Date();
const twoDaysAgo = new Date(now);
twoDaysAgo.setDate(now.getDate() - 2);

let entries: DictationHistoryEntry[] = [
  {
    id: "speech-demo-1",
    created_at: now.toISOString(),
    device: "CUDA",
    analytics_included: true,
    text: "Давайте согласуем план и следующий шаг.",
    original_text: "Ну, давайте давайте согласуем план, то есть следующий шаг.",
    analysis_status: "ready",
    analysis: {
      word_count: 9,
      filler_count: 1,
      filler_density_per_100_words: 11.1,
      repetition_count: 1,
      self_correction_count: 1,
      unfinished_count: 0,
      findings: [
        {
          kind: "filler",
          label: "Слово-паразит",
          fragment: "Ну",
          start_word: 0,
          end_word: 0,
        },
        {
          kind: "repetition",
          label: "Повтор",
          fragment: "давайте давайте",
          start_word: 1,
          end_word: 2,
        },
        {
          kind: "self_correction",
          label: "Самопоправка",
          fragment: "то есть",
          start_word: 5,
          end_word: 6,
        },
      ],
    },
    analysis_error: null,
    recommendation_status: "ready",
    recommendation: {
      summary:
        "Темп и структура хорошие; в начале заметны лишние вводные слова.",
      recommendations: [
        {
          title: "Начинайте с сути",
          observation: "Перед основной мыслью появилась вводная связка.",
          exercise: "Перед записью сформулируйте первую фразу из пяти слов.",
          finding_indexes: [0],
        },
      ],
    },
    recommendation_error: null,
  },
  {
    id: "speech-demo-2",
    created_at: twoDaysAgo.toISOString(),
    device: "CUDA",
    analytics_included: true,
    text: "Подготовлю отчёт к следующей встрече.",
    original_text: "Подготовлю отчёт к следующей встрече…",
    analysis_status: "ready",
    analysis: {
      word_count: 6,
      filler_count: 0,
      filler_density_per_100_words: 0,
      repetition_count: 0,
      self_correction_count: 0,
      unfinished_count: 1,
      findings: [
        {
          kind: "unfinished",
          label: "Незавершённая фраза",
          fragment: "встрече…",
          start_word: 5,
          end_word: 5,
        },
      ],
    },
    analysis_error: null,
    recommendation_status: "disabled",
    recommendation: null,
    recommendation_error: null,
  },
];

function report(from: string, to: string): SpeechPeriodReport {
  const includedEntries = entries.filter(
    (entry) => entry.analytics_included && entry.analysis,
  );
  const totals = includedEntries.reduce(
    (result, entry) => {
      const analysis = entry.analysis!;
      result.words += analysis.word_count;
      result.fillers += analysis.filler_count;
      result.repetitions += analysis.repetition_count;
      result.selfCorrections += analysis.self_correction_count;
      result.unfinished += analysis.unfinished_count;
      return result;
    },
    {
      words: 0,
      fillers: 0,
      repetitions: 0,
      selfCorrections: 0,
      unfinished: 0,
    },
  );

  return {
    from,
    to,
    analyzed_sessions: includedEntries.length,
    total_words: totals.words,
    filler_count: totals.fillers,
    repetition_count: totals.repetitions,
    self_correction_count: totals.selfCorrections,
    unfinished_count: totals.unfinished,
    filler_density_per_100_words:
      totals.words > 0 ? (totals.fillers * 100) / totals.words : 0,
    daily: [
      {
        date: twoDaysAgo.toISOString().slice(0, 10),
        sessions: 1,
        words: 6,
        filler_count: 0,
        repetition_count: 0,
        self_correction_count: 0,
        unfinished_count: 1,
      },
      {
        date: now.toISOString().slice(0, 10),
        sessions: 1,
        words: 9,
        filler_count: 1,
        repetition_count: 1,
        self_correction_count: 1,
        unfinished_count: 0,
      },
    ],
  };
}

export function createMockSpeechTrainerStore(): SpeechTrainerStore {
  return {
    loadHistory: async () => entries,
    loadReport: async (from, to) => report(from, to),
    loadSettings: async () =>
      ({ ...DEFAULT_SETTINGS, analytics_enabled: true }) as Settings,
    clearHistory: async () => {
      entries = [];
    },
    setSessionAnalyticsIncluded: async (id, included) => {
      entries = entries.map((entry) =>
        entry.id === id ? { ...entry, analytics_included: included } : entry,
      );
    },
    subscribe: () => () => undefined,
  };
}
