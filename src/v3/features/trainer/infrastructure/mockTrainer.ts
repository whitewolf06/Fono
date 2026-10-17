import type {
  TrainerPort,
  WorkspaceState,
} from "../../../shared/domain/contracts";
import { observeTrainerDay } from "./dayChange";
import { buildMockAiContext } from "./mockAiContext";
import {
  buildTrainerSnapshot,
  demoFingerprint,
  trainerFingerprint,
} from "../domain/period";
import type {
  TrainerAiConfiguration,
  TrainerAiSummary,
  TrainerPeriod,
  TrainerReport,
  TrainerSnapshot,
} from "../domain/report";

export function createMockTrainer(
  state: WorkspaceState,
  now: () => number = Date.now,
  wait: () => Promise<void> = () =>
    new Promise((resolve) => setTimeout(resolve, 450)),
) {
  const snapshots = new Map<TrainerPeriod, TrainerSnapshot>();
  const summaries = new Map<TrainerPeriod, TrainerAiSummary>();
  let operation = 0;
  let running = false;
  let privacyGeneration = 0;
  const privacySettings = () =>
    JSON.stringify([
      state.preferences.historyEnabled,
      state.preferences.analyticsConsent,
      state.preferences.trainerEnabled,
      state.preferences.retentionDays,
    ]);
  let privacySignature = privacySettings();
  function configuration(): TrainerAiConfiguration {
    const preferences = state.preferences;
    const profile = state.profiles.find(
      (item) => item.id === preferences.trainerProfile,
    );
    return {
      enabled: preferences.trainerAiEnabled,
      profileId: profile?.id ?? null,
      profileName: profile?.name ?? null,
      model: preferences.trainerModel || profile?.model || null,
      cloud: profile?.location === "cloud",
      cloudConsent: preferences.cloudConsent,
      dataScope: preferences.trainerScope,
    };
  }
  function aiFingerprint(
    snapshot: TrainerSnapshot,
    scope: TrainerAiSummary["dataScope"],
  ) {
    const config = configuration();
    const profile = state.profiles.find((item) => item.id === config.profileId);
    return demoFingerprint([
      snapshot.fingerprint,
      config,
      profile?.url,
      scope,
      "period-coach-1",
      "bounded-context-1",
    ]);
  }
  function reset() {
    operation++;
    privacyGeneration = 0;
    privacySignature = privacySettings();
    snapshots.clear();
    summaries.clear();
  }
  function invalidate() {
    operation++;
    privacyGeneration++;
    for (const [days, snapshot] of snapshots)
      snapshots.set(days, {
        ...snapshot,
        terms: [],
        sessionIds: [],
      });
    summaries.clear();
  }
  async function getReport(
    days: TrainerPeriod,
    refresh = false,
  ): Promise<TrainerReport> {
    if (![7, 30, 90].includes(days))
      throw new Error("Выберите период 7, 30 или 90 дней.");
    const time = now();
    const privacy = privacySettings();
    if (privacy !== privacySignature) {
      invalidate();
      privacySignature = privacy;
    }
    let snapshot = snapshots.get(days);
    // Privacy changes discard cached text, including examples and AI summaries.
    const privateSourceChanged =
      snapshot &&
      snapshot.sessionIds.some(
        (id) =>
          !state.history.some(
            (entry) =>
              entry.id === id &&
              entry.analyticsIncluded !== false &&
              entry.original &&
              Date.parse(entry.createdAt) >=
                time - state.preferences.retentionDays * 86400000,
          ),
      );
    if (privateSourceChanged) {
      invalidate();
      snapshot = snapshots.get(days);
    }
    const currentFingerprint =
      trainerFingerprint(state.history, state.preferences, days, time) +
      ":privacy-" +
      privacyGeneration;
    if (!snapshot || (refresh && snapshot.fingerprint !== currentFingerprint)) {
      snapshot = buildTrainerSnapshot(
        state.history,
        state.preferences,
        days,
        time,
      );
      snapshot.fingerprint = currentFingerprint;
      snapshots.set(days, snapshot);
      summaries.delete(days);
    }
    const stale = snapshot.fingerprint !== currentFingerprint;
    const config = configuration();
    const summary = summaries.get(days) ?? null;
    const identity = aiFingerprint(
      snapshot,
      summary?.dataScope ?? "metrics_only",
    );
    let reason: string | null = null;
    if (
      !state.preferences.analyticsConsent ||
      !state.preferences.trainerEnabled
    )
      reason = "Включите тренера и локальную аналитику в настройках.";
    else if (stale) reason = "Сначала обновите локальный отчёт.";
    else if (!snapshot.includedSessions)
      reason = "В периоде нет диктовок для анализа.";
    else if (snapshot.insufficientData)
      reason = "Для AI-разбора нужны хотя бы 3 диктовки и 100 слов.";
    else if (!config.enabled)
      reason = "Включите рекомендации ИИ в настройках тренера.";
    else if (!config.profileId || !config.model)
      reason = "Выберите подключение и модель тренера.";
    else if (config.cloud && !config.cloudConsent)
      reason = "Подтвердите передачу данных в облако в настройках.";
    return JSON.parse(
      JSON.stringify({
        snapshot: stale ? { ...snapshot, terms: [], sessionIds: [] } : snapshot,
        currentFingerprint,
        stale,
        aiConfiguration: config,
        aiFingerprint: identity,
        aiStale: stale || Boolean(summary && summary.fingerprint !== identity),
        aiSummary: stale ? null : summary,
        aiDisabledReason: reason,
        aiContext: stale
          ? null
          : {
              metricsOnly: buildMockAiContext(snapshot, [], "metrics_only"),
              configuredShared:
                config.dataScope === "metrics_only"
                  ? null
                  : buildMockAiContext(
                      snapshot,
                      state.history.filter((entry) =>
                        snapshot!.sessionIds.includes(entry.id),
                      ),
                      config.dataScope,
                    ),
            },
      }),
    ) as TrainerReport;
  }
  const port: TrainerPort = {
    onDayChange: (handler) => observeTrainerDay(handler, now),
    getReport,
    async analyzePeriod(input) {
      if (running) throw new Error("Дождитесь завершения текущего разбора.");
      // Match native: reserve the request before its first asynchronous step.
      const current = ++operation;
      running = true;
      try {
        const report = await getReport(input.days);
        if (current !== operation) throw new Error("Разбор отменён.");
        if (report.snapshot.fingerprint !== input.fingerprint || report.stale)
          throw new Error("Данные изменились. Обновите отчёт.");
        if (report.aiDisabledReason) throw new Error(report.aiDisabledReason);
        const scope = input.shareText
          ? report.aiConfiguration.dataScope
          : "metrics_only";
        const identity = aiFingerprint(report.snapshot, scope);
        if (report.aiSummary?.fingerprint === identity) return report;
        await wait();
        if (current !== operation) throw new Error("Разбор отменён.");
        const fresh = await getReport(input.days);
        if (
          fresh.stale ||
          fresh.currentFingerprint !== input.fingerprint ||
          aiFingerprint(fresh.snapshot, scope) !== identity
        )
          throw new Error(
            "Данные или настройки изменились. Результат не сохранён.",
          );
        if (!state.aiAvailable)
          throw new Error(
            "Демонстрационная ошибка AI. Локальный отчёт сохранён.",
          );
        summaries.set(input.days, {
          fingerprint: identity,
          snapshotFingerprint: input.fingerprint,
          generatedAt: new Date(now()).toISOString(),
          profileId: report.aiConfiguration.profileId!,
          profileName: report.aiConfiguration.profileName!,
          model: report.aiConfiguration.model!,
          dataScope: scope,
          context:
            scope === "metrics_only"
              ? report.aiContext!.metricsOnly
              : report.aiContext!.configuredShared,
          summary:
            "Демонстрационный разбор: начните с одной речевой привычки и сравните следующие диктовки.",
          priorities: [
            {
              title: "Пауза перед продолжением мысли",
              basis: `Локальный отчёт: слова-паразиты — ${report.snapshot.counts.filler}, повторы — ${report.snapshot.counts.repetition}.`,
              exercise:
                "Произнесите короткую мысль, сделайте паузу и только затем продолжайте. Это демонстрационный совет, внешний AI не вызывался.",
            },
          ],
        });
        return await getReport(input.days);
      } finally {
        running = false;
      }
    },
    async cancelPeriod() {
      operation++;
    },
    clear() {
      invalidate();
      state.history = state.history.map(
        ({ original: _original, findings: _findings, ...entry }) => ({
          ...entry,
          analysisReady: false,
        }),
      );
    },
  };
  return {
    port,
    reset,
    invalidate,
    dispose: () => {
      operation++;
    },
  };
}
