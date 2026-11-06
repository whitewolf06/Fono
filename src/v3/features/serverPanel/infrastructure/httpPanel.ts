import {
  isRunnable,
  validClientName,
  type PanelPort,
  type PanelRequest,
} from "../domain/panel";

export function preparePanelDocument(onExit: () => void) {
  document.documentElement.dataset.wlTheme = "graphite";
  globalThis.addEventListener("pagehide", onExit);
}

export function createHttpPanel(
  fetcher: typeof fetch = globalThis.fetch.bind(globalThis),
): PanelPort {
  async function perform(
    method: string,
    path: string,
    headers: Headers,
    signal: AbortSignal,
    body?: BodyInit,
    timeoutMs = 10000,
  ) {
    const started = performance.now();
    const timeout = new AbortController();
    const abort = () => timeout.abort();
    signal.addEventListener("abort", abort, { once: true });
    if (signal.aborted) abort();
    const timer = globalThis.setTimeout(abort, timeoutMs);
    try {
      const response = await fetcher(path, {
        method,
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
        /* Subtitle responses stay text. */
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
  }
  return {
    connection: globalThis.location?.protocol === "https:" ? "HTTPS" : "HTTP",
    now: () => Date.now(),
    async copyText(value) {
      if (!globalThis.navigator?.clipboard?.writeText)
        throw new Error("Clipboard unavailable");
      await globalThis.navigator.clipboard.writeText(value);
    },
    async requestClients(request, token, signal) {
      const fields = Object.keys(request);
      const read = request.method === "GET";
      if (
        !["GET", "POST", "DELETE"].includes(request.method) ||
        fields.some((key) => key !== "method" && key !== "name") ||
        (read
          ? fields.includes("name")
          : !("name" in request) || !validClientName(request.name))
      )
        throw new Error("Unsupported client request");
      const headers = new Headers({ Authorization: `Bearer ${token}` });
      let body: string | undefined;
      let path = "/v1/clients";
      if (request.method === "POST") {
        headers.set("Content-Type", "application/json");
        body = JSON.stringify({ name: request.name });
      } else if (request.method === "DELETE") path += `/${request.name}`;
      return perform(request.method, path, headers, signal, body);
    },
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
      return perform(
        request.method,
        path,
        headers,
        signal,
        body,
        request.method === "POST" ? 600000 : 10000,
      );
    },
  };
}
