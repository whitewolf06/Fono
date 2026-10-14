import {
  configuredFeedbackUrl,
  feedbackContext,
  feedbackUrl,
  validateFeedbackUrl,
  type FeedbackChannelPort,
} from "../domain/channel";

export function createBrowserFeedbackChannel(
  configured = configuredFeedbackUrl,
): FeedbackChannelPort {
  const context = feedbackContext(__FONO_FRONTEND_BUILD__.version, false);
  return {
    available: !!validateFeedbackUrl(configured, import.meta.env.DEV),
    context,
    async open(includeContext) {
      const url = feedbackUrl(
        configured,
        includeContext ? context : undefined,
        import.meta.env.DEV,
      );
      window.open(url, "_blank", "noopener,noreferrer");
    },
  };
}
