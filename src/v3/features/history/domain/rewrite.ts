export type RewriteSource = "original" | "result";
export type RewriteStyle =
  "preserve" | "concise" | "restrained" | "friendly" | "lively";
export const rewriteStyles: RewriteStyle[] = [
  "preserve",
  "concise",
  "restrained",
  "friendly",
  "lively",
];
export interface RewriteProfile {
  id: string;
  name: string;
  model: string;
  revision: string;
  cloud: boolean;
  configured: boolean;
}
export interface RewriteVariant {
  id: string;
  entryId: string;
  source: RewriteSource;
  sourceFingerprint: string;
  style: RewriteStyle;
  profileId: string;
  profileName: string;
  profileRevision: string;
  model: string;
  createdAt: string;
  status: "ready";
  text: string;
  stale: boolean;
}
export interface RewriteContext {
  enabled: boolean;
  profiles: RewriteProfile[];
  defaultProfileId: string | null;
  originalFingerprint: string | null;
  resultFingerprint: string | null;
  externalDelivery: "unknown";
  variants: RewriteVariant[];
}
export interface RewriteRequest {
  operationId: string;
  entryId: string;
  source: RewriteSource;
  sourceFingerprint: string;
  style: RewriteStyle;
  profileId: string;
  profileRevision: string;
  cloudConsent: boolean;
}
export interface HistoryRewritePort {
  createOperationId(): string;
  context(entryId: string): Promise<RewriteContext>;
  run(request: RewriteRequest): Promise<RewriteVariant>;
  cancel(operationId: string): Promise<void>;
  remove(entryId: string, variantId: string): Promise<void>;
}
