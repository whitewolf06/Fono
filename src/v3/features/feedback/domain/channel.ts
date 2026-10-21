import { message as uiMessage } from "../../../shared/domain/messages";
import channel from "../../../../../src-tauri/feedback-channel.json";

export interface FeedbackContext {
  version: string;
  platform: "windows" | "browser-mock";
  route: "/settings/diagnostics";
}
export interface FeedbackChannelPort {
  available: boolean;
  context: Readonly<FeedbackContext>;
  open(includeContext: boolean): Promise<void>;
}
const sourcePath =
  /^\/feedback\/submit\/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
export const configuredFeedbackUrl = channel.publicFormUrl;

export function validateFeedbackUrl(
  value: string,
  allowLoopback = false,
): URL | null {
  if (
    !value ||
    value.length > 2048 ||
    value.includes("?") ||
    value.includes("#") ||
    /\s/.test(value) ||
    [...value].some(
      (c) => c.charCodeAt(0) < 32 || c.charCodeAt(0) === 127 || c === "\\",
    )
  )
    return null;
  try {
    const url = new URL(value);
    const loopback = ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname);
    if (
      (url.protocol !== "https:" &&
        !(allowLoopback && url.protocol === "http:" && loopback)) ||
      url.username ||
      url.password ||
      url.search ||
      url.hash ||
      !sourcePath.test(url.pathname) ||
      url.href !== value
    )
      return null;
    return url;
  } catch {
    return null;
  }
}
export function feedbackContext(
  version: string,
  native: boolean,
): Readonly<FeedbackContext> {
  return Object.freeze({
    version,
    platform: native ? "windows" : "browser-mock",
    route: "/settings/diagnostics",
  });
}
export function feedbackUrl(
  configured: string,
  context?: Readonly<FeedbackContext>,
  allowLoopback = false,
): string {
  const url = validateFeedbackUrl(configured, allowLoopback);
  if (!url) throw new Error(uiMessage("ui.87bcbf779f"));
  if (context) {
    url.hash = new URLSearchParams({
      version: context.version,
      platform: context.platform,
      route: context.route,
    }).toString();
  }
  return url.href;
}
