import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { AddFilesResult, FilesPort } from "../domain/files";
type Call = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
export function createNativeFiles(
  call: Call,
  refresh: () => Promise<void>,
): FilesPort {
  async function add(paths?: string[]): Promise<AddFilesResult> {
    const result = await call<AddFilesResult>(
      paths ? "submit_transcription_files" : "choose_transcription_files",
      paths ? { paths } : undefined,
    );
    await refresh();
    return result;
  }
  return {
    choose: () => add(),
    async cancel(id) {
      await call("cancel_local_transcription_job", { id });
      await refresh();
    },
    export: (id, format) =>
      call<boolean>("export_transcription_file", { id, format }),
    async bindDrop(handlers) {
      return getCurrentWebview().onDragDropEvent((event) => {
        handlers.hover(
          event.payload.type === "enter" || event.payload.type === "over",
        );
        if (event.payload.type === "drop")
          void add(event.payload.paths)
            .then(handlers.added)
            .catch(handlers.error);
      });
    },
  };
}
