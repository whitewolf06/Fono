import { isRunnable, type PanelPort, type PanelRequest } from "../domain/panel";

export function preparePanelDocument(onExit: () => void) {
  document.documentElement.dataset.wlTheme = "graphite";
  globalThis.addEventListener("pagehide", onExit);
}

export function createHttpPanel(
  fetcher: typeof fetch = globalThis.fetch.bind(globalThis),
): PanelPort {
  return {
    connection: globalThis.location?.protocol === "https:" ? "HTTPS" : "HTTP",
    now: () => Date.now(),
    schedule(callback, ms) {
      const timer = globalThis.setTimeout(callback, ms);
      return () => globalThis.clearTimeout(timer);
    },
    chooseAudio: () =>
      new Promise((resolve) => {
        const input = document.createElement("input");
        input.type = "file";
        input.accept = ".wav,.ogg,.oga,.mp3,.flac,audio/*";
        input.addEventListener(
          "change",
          () => {
            const file = input.files?.item(0);
            resolve(
              file ? { name: file.name, size: file.size, source: file } : null,
            );
          },
          { once: true },
        );
        input.addEventListener("cancel", () => resolve(null), { once: true });
        input.click();
      }),
    async request(request: PanelRequest, token, signal, audio) {
      if (!isRunnable(request.method, request.path))
        throw new Error("Unsupported panel request");
      let path = request.path;
      const headers = new Headers({ Authorization: `Bearer ${token}` });
      let body: BodyInit | undefined;
      if (request.method === "POST") {
        if (!audio || !(audio.source instanceof Blob))
          throw new Error("Choose an audio file");
        const model = request.model ?? "";
        const language = request.language ?? "auto";
        if (path === "/v1/recognize") {
          const extension = audio.name.split(".").pop()?.toLowerCase();
          if (!["wav", "ogg", "oga"].includes(extension ?? ""))
            throw new Error("Native recognition accepts WAV or Ogg/Opus");
          headers.set(
            "Content-Type",
            extension === "wav" ? "audio/wav" : "audio/ogg;codecs=opus",
          );
          const parameters = new URLSearchParams({
            language,
            timestamps: request.timestamps ?? "none",
          });
          if (model) parameters.set("model", model);
          path += `?${parameters}`;
          body = audio.source;
        } else {
          if (!model) throw new Error("Choose an installed model");
          const form = new FormData();
          form.set("file", audio.source, audio.name);
          form.set("model", model);
          if (language !== "auto") form.set("language", language);
          form.set("response_format", request.format ?? "json");
          body = form;
        }
      }
      const started = performance.now();
      const timeout = new AbortController();
      const abort = () => timeout.abort();
      signal.addEventListener("abort", abort, { once: true });
      if (signal.aborted) abort();
      const timer = globalThis.setTimeout(
        abort,
        request.method === "POST" ? 600000 : 10000,
      );
      try {
        const response = await fetcher(path, {
          method: request.method,
          headers,
          body,
          signal: timeout.signal,
          credentials: "omit",
          mode: "same-origin",
          redirect: "error",
          cache: "no-store",
          referrerPolicy: "no-referrer",
        });
        const text = await response.text();
        let data: unknown = null;
        try {
          data = JSON.parse(text);
        } catch {
          /* Text/subtitle responses stay text. */
        }
        const retry = Number(response.headers.get("Retry-After"));
        return {
          status: response.status,
          data,
          text,
          requestId: response.headers.get("X-Request-Id") ?? "",
          retryAfter: Number.isFinite(retry)
            ? Math.max(1, Math.min(86400, retry))
            : 1,
          elapsedMs: Math.round(performance.now() - started),
        };
      } finally {
        globalThis.clearTimeout(timer);
        signal.removeEventListener("abort", abort);
      }
    },
  };
}
