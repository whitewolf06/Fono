import { message as uiMessage } from "../../../shared/domain/messages";
import type { TrainerDay } from "../domain/report";
import type { ChartMetric } from "./overview";
import { findingTitles } from "../domain/analysis";

export const chartCategories = [
  { key: "filler", title: findingTitles[0] },
  { key: "repetition", title: findingTitles[1] },
  { key: "selfCorrection", title: findingTitles[2] },
  { key: "unfinished", title: findingTitles[3] },
] as const;

export const chartMetricOptions = [
  {
    get label() {
      return uiMessage("ui.737f7561bd");
    },
    value: "count",
  },
  {
    get label() {
      return uiMessage("ui.f6da67615a");
    },
    value: "density",
  },
];

/** Label a few evenly spaced columns without dropping any underlying days. */
export function chartTickIndices(length: number, maximum = 5): number[] {
  const count = Math.min(Math.max(0, Math.floor(length)), Math.floor(maximum));
  if (count <= 0) return [];
  if (count === 1) return [0];
  return Array.from({ length: count }, (_, index) =>
    Math.round((index * (length - 1)) / (count - 1)),
  );
}

export function chartColumnOffset(index: number, length: number): number {
  return ((index + 0.5) * 100) / Math.max(1, length);
}

export function dailyChartColumns(daily: TrainerDay[], metric: ChartMetric) {
  return daily.map((day, index) => {
    let total = 0;
    const segments = chartCategories.map((category) => {
      const count = day.counts[category.key];
      const value =
        metric === "density"
          ? day.words > 0
            ? (count * 100) / day.words
            : 0
          : count;
      const segment = { ...category, count, value, start: total };
      total += value;
      return segment;
    });
    return {
      day,
      segments,
      total,
      offset: chartColumnOffset(index, daily.length),
    };
  });
}

export function preferredDayIndex(daily: TrainerDay[]): number {
  for (let index = daily.length - 1; index >= 0; index--)
    if (daily[index].sessions > 0) return index;
  return daily.length - 1;
}
