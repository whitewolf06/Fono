import { call } from "../../../shared/infrastructure/native/ipc";
import {
  configuredFeedbackUrl,
  feedbackContext,
  validateFeedbackUrl,
  type FeedbackChannelPort,
} from "../domain/channel";

export function createNativeFeedbackChannel(
  configured = configuredFeedbackUrl,
): FeedbackChannelPort {
  const available = !!validateFeedbackUrl(configured, import.meta.env.DEV);
  return {
    available,
    context: feedbackContext(__FONO_FRONTEND_BUILD__.version, true),
    async open(includeContext) {
      if (!available)
        throw new Error("Форма обратной связи пока не настроена.");
      // Native constructs the configured URL and context itself. IPC accepts no URL.
      await call("open_feedback_form", { includeContext });
    },
  };
}
