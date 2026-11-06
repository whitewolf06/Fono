import { reactive } from "vue";
import { message } from "../../../shared/domain/messages";
import type {
  RecognitionSourcePort,
  RecognitionSourceSettings,
  RemoteConnectionResult,
  RemoteServerInfo,
} from "../../../shared/domain/recognitionSource";

export function connectionErrorText(
  error: Extract<RemoteConnectionResult, { kind: "error" }>["error"],
): string {
  const keys = {
    invalid_endpoint: "recognitionSource.invalidEndpoint",
    not_configured: "recognitionSource.notConfigured",
    network: "recognitionSource.network",
    timeout: "recognitionSource.timeout",
    unauthorized: "recognitionSource.unauthorized",
    incompatible_version: "recognitionSource.incompatible",
    unsupported_capabilities: "recognitionSource.unsupportedCapabilities",
    model_not_found: "recognitionSource.modelMissing",
    invalid_response: "recognitionSource.invalidResponse",
    busy: "recognitionSource.busy",
    server_failed: "recognitionSource.serverFailed",
  } as const;
  return message(keys[error]);
}

/** Draft checks are observations. They never save settings or select a source. */
export function createRecognitionSourceController(options: {
  settings(): RecognitionSourceSettings;
  port?: RecognitionSourcePort;
  checked?(server: RemoteServerInfo | null): void;
}) {
  const state = reactive({
    token: "",
    hasToken: null as boolean | null,
    checking: false,
    tokenBusy: false,
    tokenError: "",
    tokenNotice: "" as "" | "saved" | "deleted",
    result: null as RemoteConnectionResult | null,
    server: null as RemoteServerInfo | null,
  });
  let revision = 0;
  let statusRevision = 0;
  let disposed = false;
  function signature() {
    const settings = options.settings();
    return JSON.stringify([
      settings.source,
      settings.url,
      settings.caPem,
      settings.model,
      state.token,
    ]);
  }
  function invalidate(clearInventory = true) {
    revision++;
    state.checking = false;
    state.result = null;
    if (clearInventory) {
      state.server = null;
      options.checked?.(null);
    }
  }
  return {
    state,
    invalidate,
    setToken(value: string) {
      state.token = value;
      state.tokenNotice = "";
      state.tokenError = "";
      invalidate();
    },
    async loadStatus() {
      const ticket = ++statusRevision;
      if (!options.port) return;
      try {
        const value = await options.port.status();
        if (!disposed && ticket === statusRevision)
          state.hasToken = value.hasToken;
      } catch {
        if (!disposed && ticket === statusRevision)
          state.tokenError = message("recognitionSource.tokenStatusFailed");
      }
    },
    async check() {
      const port = options.port;
      if (disposed || state.checking || !port) return;
      const ticket = ++revision;
      const submitted = signature();
      const draft = options.settings();
      const selectedModel = draft.model;
      const input = {
        url: draft.url,
        ...(draft.caPem.trim() ? { caPem: draft.caPem } : {}),
        ...(state.token ? { token: state.token } : {}),
      };
      state.result = null;
      state.checking = true;
      try {
        const result = await port.test({
          ...input,
          ...(selectedModel ? { model: selectedModel } : {}),
        });
        if (disposed || ticket !== revision || submitted !== signature())
          return;
        state.result = result;
        state.server = result.kind === "ok" ? result.server : null;
        if (
          result.kind === "ok" &&
          selectedModel &&
          !result.server.models.some(
            (model) => model.id === selectedModel && model.installed,
          )
        )
          state.result = { kind: "error", error: "model_not_found" };
        options.checked?.(state.result.kind === "ok" ? state.server : null);
        if (
          selectedModel &&
          result.kind === "error" &&
          ["model_not_found", "unsupported_capabilities"].includes(result.error)
        ) {
          // A rejected selection must not become a successful connection.
          // Read inventory separately so the user can choose a replacement.
          try {
            const inventory = await port.test(input);
            if (disposed || ticket !== revision || submitted !== signature())
              return;
            if (inventory.kind === "ok") state.server = inventory.server;
          } catch {
            // Keep the selected model's original failure if inventory fails.
          }
        }
      } catch {
        if (!disposed && ticket === revision && submitted === signature()) {
          state.result = { kind: "error", error: "network" };
          state.server = null;
          options.checked?.(null);
        }
      } finally {
        if (ticket === revision) state.checking = false;
      }
    },
    async saveToken() {
      if (disposed || state.tokenBusy || !state.token || !options.port) return;
      const value = state.token;
      if (value.length > 4096 || !/^[A-Za-z0-9._~+/=-]+$/.test(value)) {
        state.tokenError = message("recognitionSource.invalidToken");
        return;
      }
      ++statusRevision;
      state.tokenBusy = true;
      state.tokenError = "";
      state.tokenNotice = "";
      try {
        await options.port.saveToken(value);
        if (disposed) return;
        state.hasToken = true;
        // The just-tested credential stays valid as an observation. Never
        // put the secret in persisted preferences or a check-result object.
        if (state.token === value) state.token = "";
        state.tokenNotice = "saved";
      } catch {
        if (!disposed)
          state.tokenError = message("recognitionSource.tokenSaveFailed");
      } finally {
        if (!disposed) state.tokenBusy = false;
      }
    },
    async clearToken() {
      if (disposed || state.tokenBusy || !options.port) return;
      ++statusRevision;
      state.tokenBusy = true;
      state.tokenError = "";
      state.tokenNotice = "";
      try {
        await options.port.clearToken();
        if (disposed) return;
        state.token = "";
        state.hasToken = false;
        invalidate();
        state.tokenNotice = "deleted";
      } catch {
        if (!disposed)
          state.tokenError = message("recognitionSource.tokenDeleteFailed");
      } finally {
        if (!disposed) state.tokenBusy = false;
      }
    },
    dispose() {
      disposed = true;
      ++revision;
      ++statusRevision;
      state.token = "";
    },
  };
}
