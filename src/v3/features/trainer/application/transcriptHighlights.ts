import type { Finding } from "../domain/analysis";
import { findingTitles } from "../domain/analysis";
import { chartCategories } from "./chart";

export type TranscriptMarkerCategory = (typeof chartCategories)[number]["key"];

export interface TranscriptMarker {
  number: number;
  category: TranscriptMarkerCategory;
  title: string;
}

export interface TranscriptHighlight {
  text: string;
  highlighted: boolean;
  category?: TranscriptMarkerCategory;
  /** Saved occurrences ending here; separate categories may overlap. */
  markers?: TranscriptMarker[];
}

interface Word {
  value: string;
  start: number;
  end: number;
}

interface Pattern {
  words: string[];
  remaining: number;
}

interface HighlightRange {
  start: number;
  end: number;
  category: TranscriptMarkerCategory;
  title: string;
}

/** Present only saved finding examples; do not run or extend speech rules. */
export function transcriptHighlights(
  text: string,
  findings: readonly Pick<Finding, "title" | "count" | "example">[],
): TranscriptHighlight[] {
  // Numbers and underscores stay inside lexical units, so a saved "ну" does
  // not paint substrings such as "ну42" or "ну_ка" as whole words.
  const words: Word[] = Array.from(
    text.matchAll(/[\p{L}\p{M}\p{N}_]+/gu),
    (match) => ({
      value: match[0].toLocaleLowerCase("ru-RU"),
      start: match.index,
      end: match.index + match[0].length,
    }),
  );
  const ranges: HighlightRange[] = [];

  for (const finding of findings) {
    const category = chartCategories.find(
      (item) => item.title === finding.title,
    );
    if (!category) continue;
    if (!Number.isFinite(finding.count) || finding.count <= 0) continue;
    let remaining = Math.floor(finding.count);
    const patterns = new Map<string, Pattern>();
    for (const example of finding.example.split(",")) {
      const parts = example.trim().split(/\s+/u);
      if (!parts.every((part) => /^\p{L}[\p{L}\p{M}]*$/u.test(part))) continue;
      const normalized = parts.map((part) => part.toLocaleLowerCase("ru-RU"));
      const key = normalized.join(" ");
      const existing = patterns.get(key);
      if (existing) existing.remaining++;
      else patterns.set(key, { words: normalized, remaining: 1 });
    }
    const unfinished = finding.title === findingTitles[3];
    if (unfinished && !/(?:…|\.\.\.|-)\s*$/u.test(text)) continue;
    const byFirstWord = new Map<string, Pattern[]>();
    for (const pattern of patterns.values()) {
      const candidates = byFirstWord.get(pattern.words[0]) || [];
      candidates.push(pattern);
      byFirstWord.set(pattern.words[0], candidates);
    }
    for (let index = 0; index < words.length && remaining > 0; index++) {
      for (const pattern of byFirstWord.get(words[index].value) || []) {
        const end = index + pattern.words.length;
        if (
          pattern.remaining <= 0 ||
          end > words.length ||
          (unfinished && end !== words.length) ||
          !pattern.words.every(
            (word, offset) => words[index + offset].value === word,
          )
        )
          continue;
        ranges.push({
          start: words[index].start,
          end: words[end - 1].end,
          category: category.key,
          title: category.title,
        });
        pattern.remaining--;
        remaining--;
        if (remaining <= 0) break;
      }
    }
  }

  if (!ranges.length) return [{ text, highlighted: false }];
  const numbered = ranges
    .sort(
      (a, b) =>
        a.end - b.end ||
        a.start - b.start ||
        a.category.localeCompare(b.category),
    )
    .map((range, index) => ({ ...range, number: index + 1 }));
  const boundaries = [
    ...new Set([
      0,
      text.length,
      ...numbered.flatMap((range) => [range.start, range.end]),
    ]),
  ].sort((a, b) => a - b);
  const segments: TranscriptHighlight[] = [];
  for (let index = 0; index < boundaries.length - 1; index++) {
    const start = boundaries[index];
    const end = boundaries[index + 1];
    const covering = numbered.filter(
      (range) => range.start <= start && range.end >= end,
    );
    // The shortest saved occurrence supplies the text color. Every ending
    // occurrence keeps its own numbered category, including overlapping pairs.
    const primary = covering.sort(
      (a, b) => a.end - a.start - (b.end - b.start) || a.number - b.number,
    )[0];
    const markers = covering
      .filter((range) => range.end === end)
      .sort((a, b) => a.number - b.number)
      .map(({ number, category, title }) => ({ number, category, title }));
    const segment: TranscriptHighlight = primary
      ? {
          text: text.slice(start, end),
          highlighted: true,
          category: primary.category,
          ...(markers.length ? { markers } : {}),
        }
      : { text: text.slice(start, end), highlighted: false };
    const previous = segments.at(-1);
    if (
      previous &&
      previous.highlighted === segment.highlighted &&
      previous.category === segment.category &&
      !previous.markers?.length
    ) {
      previous.text += segment.text;
      if (segment.markers) previous.markers = segment.markers;
    } else segments.push(segment);
  }
  return segments;
}
