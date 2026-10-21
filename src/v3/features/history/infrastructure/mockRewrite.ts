import type { WorkspaceState } from "../../../shared/domain/contracts";
import type {
  HistoryRewritePort,
  RewriteContext,
  RewriteRequest,
  RewriteVariant,
} from "../domain/rewrite";

async function fingerprint(value: unknown) {
  const bytes = new TextEncoder().encode(JSON.stringify(value));
  return Array.from(
    new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)),
    (b) => b.toString(16).padStart(2, "0"),
  ).join("");
}
export function createMockHistoryRewrite(state: WorkspaceState) {
  const variants = new Map<string, RewriteVariant>();
  const jobs = new Map<string, boolean>();
  let epoch = 0;
  function prune() {
    for (const [id, variant] of variants) {
      const entry = state.history.find((e) => e.id === variant.entryId);
      if (
        !entry ||
        (variant.source === "original" &&
          (!entry.original || !state.preferences.analyticsConsent))
      )
        variants.delete(id);
    }
    epoch++;
  }
  async function context(entryId: string): Promise<RewriteContext> {
    const entry = state.history.find((e) => e.id === entryId);
    if (!entry) throw new Error("missing entry");
    const profiles = await Promise.all(
      state.profiles.map(async (p) => {
        const model =
          p.id === state.preferences.profile
            ? state.preferences.processingModel || p.model
            : p.model;
        const cloud = p.location === "cloud" || p.provider === "openai";
        return {
          id: p.id,
          name: p.name,
          model,
          cloud,
          revision: await fingerprint([
            p.id,
            p.provider,
            p.location,
            p.url,
            model,
          ]),
          configured: !!model && !!p.url && (!cloud || !!p.hasApiKey),
        };
      }),
    );
    const originalFingerprint = entry.original
      ? await fingerprint([
          entry.id,
          entry.createdAt,
          "original",
          entry.original,
        ])
      : null;
    const resultFingerprint = await fingerprint([
      entry.id,
      entry.createdAt,
      "result",
      entry.text,
    ]);
    const saved = [...variants.values()]
      .filter((v) => v.entryId === entryId)
      .map((v) => ({
        ...v,
        stale:
          (v.source === "original"
            ? originalFingerprint
            : resultFingerprint) !== v.sourceFingerprint ||
          !profiles.some(
            (p) => p.id === v.profileId && p.revision === v.profileRevision,
          ),
      }));
    return {
      enabled: state.preferences.historyEnabled,
      profiles,
      defaultProfileId: state.preferences.profile || null,
      originalFingerprint,
      resultFingerprint,
      externalDelivery: "unknown",
      variants: saved.reverse(),
    };
  }
  const port: HistoryRewritePort = {
    createOperationId: () => crypto.randomUUID(),
    context,
    async run(request: RewriteRequest) {
      if (jobs.size) throw new Error("busy");
      jobs.set(request.operationId, false);
      const admittedEpoch = epoch;
      try {
        const value = await context(request.entryId);
        const profile = value.profiles.find(
          (p) =>
            p.id === request.profileId &&
            p.revision === request.profileRevision &&
            p.configured,
        );
        const source =
          request.source === "original"
            ? value.originalFingerprint
            : value.resultFingerprint;
        if (
          !value.enabled ||
          !profile ||
          !source ||
          source !== request.sourceFingerprint ||
          (profile.cloud && !request.cloudConsent)
        )
          throw new Error("stale request");
        const key = await fingerprint([
          request.entryId,
          source,
          request.style,
          profile.revision,
        ]);
        const cached = variants.get(key);
        if (cached) return { ...cached };
        const entry = state.history.find((e) => e.id === request.entryId)!;
        const material =
          request.source === "original" ? entry.original! : entry.text;
        await new Promise((resolve) => setTimeout(resolve, 160));
        if (jobs.get(request.operationId) || admittedEpoch !== epoch)
          throw new Error("cancelled");
        const latest = await context(request.entryId);
        if (
          !latest.profiles.some(
            (p) => p.id === profile.id && p.revision === profile.revision,
          ) ||
          (request.source === "original"
            ? latest.originalFingerprint
            : latest.resultFingerprint) !== source
        )
          throw new Error("stale request");
        const result: RewriteVariant = {
          id: key,
          entryId: request.entryId,
          source: request.source,
          sourceFingerprint: source,
          style: request.style,
          profileId: profile.id,
          profileName: profile.name,
          profileRevision: profile.revision,
          model: profile.model,
          createdAt: new Date().toISOString(),
          status: "ready",
          text: material,
          stale: false,
        };
        variants.set(key, result);
        return { ...result };
      } finally {
        jobs.delete(request.operationId);
      }
    },
    async cancel(id) {
      if (jobs.has(id)) jobs.set(id, true);
    },
    async remove(entryId, id) {
      if (variants.get(id)?.entryId === entryId) variants.delete(id);
      epoch++;
    },
  };
  return {
    port,
    prune,
    clear() {
      variants.clear();
      epoch++;
    },
    dispose() {
      variants.clear();
      for (const id of jobs.keys()) jobs.set(id, true);
      epoch++;
    },
  };
}
