import { message } from "../../../shared/domain/messages";
import type {
  HistoryRewritePort,
  RewriteContext,
  RewriteProfile,
  RewriteSource,
  RewriteStyle,
  RewriteVariant,
} from "../domain/rewrite";

export interface RewriteViewState {
  entryId: string;
  context: RewriteContext | null;
  profileId: string;
  source: RewriteSource;
  style: RewriteStyle;
  selectedId: string;
  loading: boolean;
  generating: boolean;
  cancelling: boolean;
  removing: boolean;
  error: string;
}
export function initialRewriteState(): RewriteViewState {
  return {
    entryId: "",
    context: null,
    profileId: "",
    source: "result",
    style: "preserve",
    selectedId: "",
    loading: false,
    generating: false,
    cancelling: false,
    removing: false,
    error: "",
  };
}
export function selectedRewriteProfile(state: RewriteViewState) {
  return state.context?.profiles.find((p) => p.id === state.profileId);
}
export function selectedRewrite(state: RewriteViewState) {
  return state.context?.variants.find((v) => v.id === state.selectedId);
}
export function matchesRewrite(
  state: RewriteViewState,
  variant: RewriteVariant,
) {
  const profile = selectedRewriteProfile(state);
  const source =
    state.source === "original"
      ? state.context?.originalFingerprint
      : state.context?.resultFingerprint;
  return (
    !variant.stale &&
    variant.source === state.source &&
    variant.sourceFingerprint === source &&
    variant.style === state.style &&
    variant.profileId === profile?.id &&
    variant.profileRevision === profile?.revision
  );
}
export function canRewrite(state: RewriteViewState) {
  const source =
    state.source === "original"
      ? state.context?.originalFingerprint
      : state.context?.resultFingerprint;
  return !!(
    state.context?.enabled &&
    source &&
    selectedRewriteProfile(state)?.configured &&
    !state.loading &&
    !state.generating &&
    !state.removing
  );
}

export function createRewriteController(
  port: HistoryRewritePort,
  state: RewriteViewState,
) {
  let generation = 0;
  let read = 0;
  let operation: string | null = null;
  let disposed = false;
  async function refresh() {
    const current = generation,
      stamp = ++read,
      entry = state.entryId;
    if (!entry || disposed) return;
    state.loading = true;
    try {
      const value = await port.context(entry);
      if (disposed || current !== generation || stamp !== read) return;
      state.context = value;
      if (!value.profiles.some((p) => p.id === state.profileId))
        state.profileId =
          value.defaultProfileId ||
          value.profiles.find((p) => p.configured)?.id ||
          value.profiles[0]?.id ||
          "";
      if (state.source === "original" && !value.originalFingerprint)
        state.source = "result";
      if (!value.variants.some((v) => v.id === state.selectedId))
        state.selectedId = value.variants[0]?.id || "";
    } catch {
      if (!disposed && current === generation && stamp === read)
        state.error = message("historyRewrite.loadFailed");
    } finally {
      if (!disposed && current === generation && stamp === read)
        state.loading = false;
    }
  }
  async function open(entry: string) {
    generation++;
    if (operation) void port.cancel(operation).catch(() => {});
    operation = null;
    Object.assign(state, initialRewriteState(), { entryId: entry });
    await refresh();
  }
  async function generate(
    confirmCloud: (profile: RewriteProfile) => Promise<boolean>,
  ) {
    if (!canRewrite(state) || disposed) return;
    const cached = state.context?.variants.find((v) =>
      matchesRewrite(state, v),
    );
    if (cached) {
      state.selectedId = cached.id;
      state.error = "";
      return;
    }
    const profile = selectedRewriteProfile(state)!;
    const current = generation;
    const id = port.createOperationId();
    operation = id;
    state.generating = true;
    state.error = "";
    try {
      const consent = !profile.cloud || (await confirmCloud(profile));
      if (
        !consent ||
        disposed ||
        current !== generation ||
        operation !== id ||
        state.cancelling
      )
        return;
      const source =
        state.source === "original"
          ? state.context!.originalFingerprint
          : state.context!.resultFingerprint;
      const value = await port.run({
        operationId: id,
        entryId: state.entryId,
        source: state.source,
        sourceFingerprint: source!,
        style: state.style,
        profileId: profile.id,
        profileRevision: profile.revision,
        cloudConsent: profile.cloud && consent,
      });
      if (
        disposed ||
        current !== generation ||
        operation !== id ||
        state.cancelling
      )
        return;
      state.context!.variants = [
        value,
        ...state.context!.variants.filter((v) => v.id !== value.id),
      ];
      state.selectedId = value.id;
    } catch {
      if (
        !disposed &&
        current === generation &&
        operation === id &&
        !state.cancelling
      )
        state.error = message("historyRewrite.failed");
    } finally {
      if (!disposed && operation === id) {
        operation = null;
        state.generating = false;
        state.cancelling = false;
      }
    }
  }
  async function cancel() {
    if (!operation) return;
    state.cancelling = true;
    try {
      await port.cancel(operation);
    } catch {
      state.error = message("historyRewrite.cancelFailed");
    }
  }
  async function remove(
    expectedEntry = state.entryId,
    expectedVariant = state.selectedId,
  ) {
    if (expectedEntry !== state.entryId || expectedVariant !== state.selectedId)
      return;
    if (!state.selectedId || state.generating || state.removing || disposed)
      return;
    const current = generation;
    state.removing = true;
    state.error = "";
    try {
      await port.remove(state.entryId, state.selectedId);
      if (!disposed && current === generation) await refresh();
    } catch {
      if (!disposed && current === generation)
        state.error = message("historyRewrite.failed");
    } finally {
      if (!disposed && current === generation) state.removing = false;
    }
  }
  return {
    open,
    refresh,
    generate,
    cancel,
    remove,
    dispose() {
      disposed = true;
      generation++;
      read++;
      if (operation) void port.cancel(operation).catch(() => {});
    },
  };
}
