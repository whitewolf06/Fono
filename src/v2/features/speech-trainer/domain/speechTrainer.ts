import type { SpeechPeriodReport } from "@/lib/types";

export type SpeechTrainerPeriod = 7 | 30 | 90;

export const speechTrainerPeriods: {
  value: SpeechTrainerPeriod;
  label: string;
}[] = [
  { value: 7, label: "7 дней" },
  { value: 30, label: "30 дней" },
  { value: 90, label: "90 дней" },
];

export function getSpeechTrainerPeriodBounds(
  days: SpeechTrainerPeriod,
  now = new Date(),
) {
  const to = new Date(now);
  const from = new Date(now);
  from.setUTCDate(from.getUTCDate() - (days - 1));
  from.setUTCHours(0, 0, 0, 0);
  return { from: from.toISOString(), to: to.toISOString() };
}

export function hasSpeechFindings(report: SpeechPeriodReport) {
  return (
    report.filler_count +
      report.repetition_count +
      report.self_correction_count +
      report.unfinished_count >
    0
  );
}

export function formatDensity(value: number) {
  return `${value.toFixed(1)} / 100`;
}
