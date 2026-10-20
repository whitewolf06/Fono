import type { Snippet, SnippetTransferPort } from "../domain/snippets";
type Call = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
export function createNativeSnippetTransfer(call: Call): SnippetTransferPort {
  return {
    import: () => call<Snippet[] | null>("import_snippets"),
    export: (entries) =>
      call<boolean>("export_snippets", {
        entries: entries.map((entry) => ({ ...entry })),
      }),
  };
}
