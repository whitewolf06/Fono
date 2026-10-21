import type { HistoryRewritePort } from "../domain/rewrite";
type NativeCall = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

export function createNativeHistoryRewrite(
  call: NativeCall,
): HistoryRewritePort {
  return {
    createOperationId: () => crypto.randomUUID(),
    context: (entryId) => call("get_history_rewrite_context", { entryId }),
    run: (request) => call("create_history_rewrite", { request }),
    cancel: (operationId) => call("cancel_history_rewrite", { operationId }),
    remove: (entryId, variantId) =>
      call("delete_history_rewrite", { entryId, variantId }),
  };
}
