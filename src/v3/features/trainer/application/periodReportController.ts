import { message as uiMessage } from "../../../shared/domain/messages";
import type { TrainerPort } from "../../../shared/domain/contracts";
import type { TrainerPeriod, TrainerReport } from "../domain/report";

export interface PeriodReportViewState {
  days: TrainerPeriod;
  report: TrainerReport | null;
  loading: boolean;
  analyzing: boolean;
  cancelling: boolean;
  error: string;
}

export function initialPeriodReportState(): PeriodReportViewState {
  return {
    days: 7,
    report: null,
    loading: false,
    analyzing: false,
    cancelling: false,
    error: "",
  };
}

export function periodConsentKey(
  report: TrainerReport,
  connectionKey = "",
): string {
  const configuration = report.aiConfiguration;
  return JSON.stringify([
    report.snapshot.fingerprint,
    connectionKey,
    configuration.enabled,
    configuration.profileId,
    configuration.profileName,
    configuration.model,
    configuration.dataScope,
    configuration.cloud,
    configuration.cloudConsent,
  ]);
}

export function periodAiDisabledReason(
  report: TrainerReport,
  shareText: boolean,
): string | null {
  if (report.stale) return uiMessage("ui.e9fa4868eb");
  if (report.aiDisabledReason) return report.aiDisabledReason;
  const desiredScope = shareText
    ? report.aiConfiguration.dataScope
    : "metrics_only";
  if (!report.aiStale && report.aiSummary?.dataScope === desiredScope)
    return uiMessage("ui.326befeaba");
  return null;
}

export function createPeriodReportController(
  port: Pick<TrainerPort, "getReport" | "analyzePeriod" | "cancelPeriod">,
  state: PeriodReportViewState,
) {
  let revision = 0;
  let disposed = false;
  let cancellation: Promise<void> = Promise.resolve();
  let cancellationFailed = false;
  const current = (ticket: number) => !disposed && ticket === revision;
  const message = (error: unknown) =>
    error instanceof Error ? error.message : uiMessage("ui.4c7361b649");

  function cancelRequest(force = false): Promise<void> {
    if (!force && !state.analyzing && !state.cancelling && !cancellationFailed)
      return cancellation;
    state.analyzing = false;
    state.cancelling = true;
    const request = cancellation
      .catch(() => undefined)
      .then(() => port.cancelPeriod());
    cancellation = request;
    void request.then(
      () => {
        if (cancellation === request) cancellationFailed = false;
      },
      () => {
        if (cancellation === request) cancellationFailed = true;
      },
    );
    return request.finally(() => {
      if (!disposed && cancellation === request) state.cancelling = false;
    });
  }

  async function load(refresh = false) {
    if (disposed) return;
    const ticket = ++revision;
    const days = state.days;
    const cancelled = cancelRequest();
    state.loading = true;
    state.error = "";
    try {
      await cancelled;
      if (!current(ticket)) return;
      const report = await port.getReport(days, refresh);
      if (current(ticket)) state.report = report;
    } catch (error) {
      if (current(ticket)) state.error = message(error);
    } finally {
      if (current(ticket)) state.loading = false;
    }
  }

  async function setPeriod(days: TrainerPeriod) {
    if (disposed || state.days === days) return;
    state.days = days;
    state.report = null;
    await load();
  }

  async function analyze(shareText: boolean) {
    const report = state.report;
    if (
      disposed ||
      state.loading ||
      state.analyzing ||
      state.cancelling ||
      !report ||
      periodAiDisabledReason(report, shareText)
    )
      return;
    const ticket = ++revision;
    const days = state.days;
    state.analyzing = true;
    state.error = "";
    try {
      await cancellation;
      if (!current(ticket)) return;
      const result = await port.analyzePeriod({
        days,
        fingerprint: report.snapshot.fingerprint,
        shareText,
      });
      if (current(ticket)) state.report = result;
    } catch (error) {
      if (current(ticket)) {
        state.error = message(error);
        try {
          const latest = await port.getReport(days, false);
          if (current(ticket)) state.report = latest;
        } catch {
          // Keep the original AI error and the last usable local snapshot.
        }
      }
    } finally {
      if (current(ticket)) state.analyzing = false;
    }
  }

  async function cancel() {
    const ticket = ++revision;
    state.loading = false;
    state.error = "";
    try {
      await cancelRequest(true);
    } catch (error) {
      if (current(ticket)) state.error = message(error);
    }
  }

  async function clear() {
    const ticket = ++revision;
    state.report = null;
    state.loading = false;
    state.error = "";
    try {
      await cancelRequest(true);
    } catch (error) {
      if (current(ticket)) state.error = message(error);
    }
  }

  async function dispose() {
    disposed = true;
    revision++;
    await cancelRequest(true).catch(() => undefined);
  }

  return { load, setPeriod, analyze, cancel, clear, dispose };
}
