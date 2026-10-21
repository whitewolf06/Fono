import { message as uiMessage } from "../../../shared/domain/messages";
import type { WorkspaceState } from "../../../shared/domain/contracts";
import type { AddFilesResult, FilesPort } from "../domain/files";
export function createMockFiles(state: WorkspaceState): FilesPort {
  let sequence = 0;
  function add(names: string[]): AddFilesResult {
    const result: AddFilesResult = { accepted: [], rejected: 0 };
    for (const name of names.slice(0, 16)) {
      if (!/\.(wav|mp3|flac|ogg)$/i.test(name)) {
        result.rejected++;
        continue;
      }
      const id = `file-demo-${++sequence}`;
      state.jobs.unshift({
        id,
        name,
        file: true,
        progress: 1,
        state: "done",
        seconds: 1800,
        text: "Демонстрационный результат. Реальные файлы распознаются в приложении Fono.",
        hasTimestamps: true,
      });
      result.accepted.push(id);
    }
    result.rejected += Math.max(0, names.length - 16);
    return result;
  }
  return {
    async choose() {
      return add(["Встреча — демо.wav", "Заметки — демо.mp3"]);
    },
    async cancel(id) {
      const job = state.jobs.find((job) => job.id === id);
      if (job && ["running", "queued"].includes(job.state))
        job.state = "cancelled";
    },
    async export(id, format) {
      const job = state.jobs.find(
        (job) => job.id === id && job.state === "done",
      );
      if (!job) throw new Error(uiMessage("ui.16b13f6e20"));
      const text = job.text || "";
      const content =
        format === "txt"
          ? text
          : format === "srt"
            ? `1\n00:00:01,000 --> 00:00:06,000\n${text}\n`
            : `WEBVTT\n\n1\n00:00:01.000 --> 00:00:06.000\n${text}\n`;
      const url = URL.createObjectURL(
        new Blob([content], { type: "text/plain;charset=utf-8" }),
      );
      const link = document.createElement("a");
      link.href = url;
      link.download = `fono-demo.${format}`;
      link.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
      return true;
    },
    async bindDrop(handlers) {
      const over = (event: DragEvent) => {
        event.preventDefault();
        handlers.hover(true);
      };
      const leave = () => handlers.hover(false);
      const drop = (event: DragEvent) => {
        event.preventDefault();
        handlers.hover(false);
        // Browser preview uses names only, never reads/recognizes private audio.
        handlers.added(
          add(
            Array.from(event.dataTransfer?.files || []).map(
              (file) => file.name,
            ),
          ),
        );
      };
      window.addEventListener("dragover", over);
      window.addEventListener("dragleave", leave);
      window.addEventListener("drop", drop);
      return () => {
        window.removeEventListener("dragover", over);
        window.removeEventListener("dragleave", leave);
        window.removeEventListener("drop", drop);
      };
    },
  };
}
