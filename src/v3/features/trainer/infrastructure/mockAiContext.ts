import type { Dictation } from "../../../shared/domain/contracts";
import type { TrainerAiContext, TrainerSnapshot } from "../domain/report";

// Browser demonstration only. Native preflight comes from the exact Rust payload builder.
const limits = {
  maxSharedSessions: 12,
  maxCharactersPerSession: 3000,
  maxPatterns: 40,
  maxExamplesPerPattern: 2,
};
const payloadByteLimit = 32768;
const encoder = new TextEncoder();
const words = (text: string) => text.match(/\p{Alphabetic}+/gu)?.length ?? 0;
const characters = (text: string) => Array.from(text).length;
function prefix(text: string, count: number) {
  const points = Array.from(text);
  let end = Math.min(count, points.length);
  if (end < points.length && /[\p{L}\p{M}\p{N}_]/u.test(points[end]))
    while (end > 0 && /[\p{L}\p{M}\p{N}_]/u.test(points[end - 1])) end--;
  return points.slice(0, end).join("");
}

export function buildMockAiContext(
  snapshot: TrainerSnapshot,
  sources: Dictation[],
  dataScope: TrainerAiContext["dataScope"],
): TrainerAiContext {
  const ranks = snapshot.terms
    .slice(0, limits.maxPatterns)
    .map((term, index) => ({
      rank: index + 1,
      kind: term.kind,
      count: term.count,
      density: term.density,
      affectedSessions: term.affectedSessions,
    }));
  const findings: {
    rank: number;
    kind: string;
    label: string;
    count: number;
    examples: string[];
  }[] = [];
  const textSample = {
    sharedSessions: 0,
    availableSessions: sources.length,
    maxCharactersPerSession: limits.maxCharactersPerSession,
    originalTexts: [] as string[],
  };
  const payload = {
    period: {
      days: snapshot.days,
      from: snapshot.from,
      to: snapshot.to,
      timezone: "UTC",
    },
    rulesVersion: snapshot.rulesVersion,
    sample: {
      includedSessions: snapshot.includedSessions,
      totalWords: snapshot.totalWords,
      excludedSessions: snapshot.excludedSessions,
      pendingSessions: snapshot.pendingSessions,
      unavailableSessions: snapshot.unavailableSessions,
      historyLimit: snapshot.historyLimit,
      retentionDays: snapshot.retentionDays,
      coverageStart: snapshot.coverageStart,
      sufficientSample: !snapshot.insufficientData,
      rankedPatternsIncluded: ranks.length,
      rankedPatternsTotal: snapshot.terms.length,
      contextByteLimit: payloadByteLimit,
      confidenceLimit:
        "Local text heuristics; transcription accuracy and causes are not established. Examples and original texts may be omitted or shortened by Fono limits; aggregate counts cover the whole included period.",
    },
    counts: snapshot.counts,
    density: snapshot.density,
    daily: snapshot.daily,
    patternRanks: ranks,
    ...(dataScope !== "metrics_only" ? { findings } : {}),
    ...(dataScope === "original_text" ? { textSample } : {}),
  };
  const bytes = () => encoder.encode(JSON.stringify(payload)).length;
  const availableExamples = snapshot.terms.flatMap((term) => term.examples);
  const sharedExamples: typeof availableExamples = [];
  if (dataScope !== "metrics_only") {
    for (const [index, term] of snapshot.terms
      .slice(0, limits.maxPatterns)
      .entries()) {
      const pattern = {
        rank: index + 1,
        kind: term.kind,
        label: term.label,
        count: term.count,
        examples: [] as string[],
      };
      findings.push(pattern);
      if (bytes() > payloadByteLimit) {
        findings.pop();
        continue;
      }
      for (const example of term.examples.slice(
        0,
        limits.maxExamplesPerPattern,
      )) {
        pattern.examples.push(example.fragment);
        if (bytes() > payloadByteLimit) pattern.examples.pop();
        else sharedExamples.push(example);
      }
    }
  }
  const orderedSources = [...sources].sort(
    (left, right) =>
      left.createdAt.localeCompare(right.createdAt) ||
      left.id.localeCompare(right.id),
  );
  const sharedOriginals: { id: string; text: string; original: string }[] = [];
  if (dataScope === "original_text") {
    for (const source of orderedSources.slice(0, limits.maxSharedSessions)) {
      const original = source.original ?? "";
      let low = 0;
      let high = Math.min(characters(original), limits.maxCharactersPerSession);
      textSample.sharedSessions = textSample.originalTexts.length + 1;
      textSample.originalTexts.push("");
      while (low < high) {
        const middle = Math.ceil((low + high) / 2);
        textSample.originalTexts[textSample.originalTexts.length - 1] = prefix(
          original,
          middle,
        );
        if (bytes() <= payloadByteLimit) low = middle;
        else high = middle - 1;
      }
      const text = prefix(original, low);
      textSample.originalTexts[textSample.originalTexts.length - 1] = text;
      if (!text || bytes() > payloadByteLimit) textSample.originalTexts.pop();
      else sharedOriginals.push({ id: source.id, text, original });
      textSample.sharedSessions = textSample.originalTexts.length;
    }
  }
  const availableTexts =
    dataScope === "original_text"
      ? sources.map((source) => source.original ?? "")
      : dataScope === "findings"
        ? availableExamples.map((example) => example.fragment)
        : [];
  const sharedTexts =
    dataScope === "original_text"
      ? sharedOriginals.map((source) => source.text)
      : dataScope === "findings"
        ? sharedExamples.map((example) => example.fragment)
        : [];
  const availableSessions =
    dataScope === "original_text"
      ? sources.length
      : dataScope === "findings"
        ? new Set(availableExamples.map((example) => example.sessionId)).size
        : 0;
  const sharedSessions =
    dataScope === "original_text"
      ? sharedOriginals.length
      : dataScope === "findings"
        ? new Set(sharedExamples.map((example) => example.sessionId)).size
        : 0;
  const shortenedSessions =
    dataScope === "original_text"
      ? sharedOriginals.filter((source) => source.text !== source.original)
          .length
      : dataScope === "findings"
        ? new Set(
            sharedExamples
              .filter(
                (example) =>
                  availableExamples.filter(
                    (item) => item.sessionId === example.sessionId,
                  ).length >
                  sharedExamples.filter(
                    (item) => item.sessionId === example.sessionId,
                  ).length,
              )
              .map((example) => example.sessionId),
          ).size
        : 0;
  const availableWords = availableTexts.reduce(
    (sum, text) => sum + words(text),
    0,
  );
  const sharedWords = sharedTexts.reduce((sum, text) => sum + words(text), 0);
  const availableCharacters = availableTexts.reduce(
    (sum, text) => sum + characters(text),
    0,
  );
  const sharedCharacters = sharedTexts.reduce(
    (sum, text) => sum + characters(text),
    0,
  );
  const patternsShared =
    dataScope === "metrics_only" ? ranks.length : findings.length;
  const examplesAvailable =
    dataScope === "metrics_only" ? 0 : availableExamples.length;
  return {
    dataScope,
    payloadBytes: bytes(),
    payloadByteLimit,
    reduced:
      patternsShared < snapshot.terms.length ||
      sharedCharacters < availableCharacters ||
      sharedExamples.length < examplesAvailable,
    metricsSessions: snapshot.includedSessions,
    metricsWords: snapshot.totalWords,
    availableSessions,
    sharedSessions,
    omittedSessions: availableSessions - sharedSessions,
    shortenedSessions,
    availableWords,
    sharedWords,
    omittedWords: availableWords - sharedWords,
    availableCharacters,
    sharedCharacters,
    omittedCharacters: availableCharacters - sharedCharacters,
    patternsAvailable: snapshot.terms.length,
    patternsShared,
    examplesAvailable,
    examplesShared: sharedExamples.length,
    limits: { ...limits },
  };
}
