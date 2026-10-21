import { message as uiMessage } from "../../../shared/domain/messages";
import type {
  PendingDictation,
  PendingDictationRequest,
} from "../../../shared/domain/processing";
interface CopyPorts {
  pending: () => PendingDictation | null | undefined;
  copy: (text: string) => Promise<void>;
  resolve: (request: PendingDictationRequest) => Promise<void>;
}
export async function copyPendingText(
  ports: CopyPorts,
  text: string,
  sessionId: number,
): Promise<void> {
  const current = ports.pending();
  if (!current || current.sessionId !== sessionId)
    throw new Error(uiMessage("ui.a2af9f2bd1"));
  if (current.phase === "processing")
    throw new Error(uiMessage("ui.77e3ca0d3b"));
  await ports.copy(text);
  if (current.copyOnly && ports.pending()?.sessionId === sessionId)
    await ports.resolve({ sessionId, action: "complete" });
}
