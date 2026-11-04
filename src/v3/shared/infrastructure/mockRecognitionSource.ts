import type {
  RecognitionSourcePort,
  RemoteConnectionError,
  RemoteServerInfo,
} from "../domain/recognitionSource";
import { validRemoteEndpoint } from "../domain/recognitionSource";
import type { WorkspaceState } from "../domain/contracts";
import { message } from "../domain/messages";
const wait = () => new Promise<void>((resolve) => setTimeout(resolve, 180));

/** Demo only: no fetch, credential vault, token retention or browser persistence. */
export function createMockRecognitionSource(
  state: WorkspaceState,
): RecognitionSourcePort {
  let hasToken = false;
  const guard = () => {
    if (
      state.triggerCaptureActive ||
      [
        "listening",
        "silence",
        "transcribing",
        "processing",
        "awaiting_action",
      ].includes(state.phase) ||
      state.recoveryDictation ||
      state.pendingDictation
    )
      throw new Error(message("ui.e134b483ff"));
  };
  return {
    async status() {
      return { hasToken };
    },
    async test(input) {
      await wait();
      if (!validRemoteEndpoint(input.url))
        return { kind: "error", error: "invalid_endpoint" };
      if (
        !input.token &&
        (!hasToken ||
          input.url !== state.preferences.recognitionSource.url ||
          (input.caPem || "") !== state.preferences.recognitionSource.caPem)
      )
        return { kind: "error", error: "not_configured" };
      const hostname = new URL(input.url).hostname;
      const scenario: RemoteConnectionError | undefined =
        input.token === "invalid"
          ? "unauthorized"
          : hostname.includes("unavailable")
            ? "network"
            : hostname.includes("timeout")
              ? "timeout"
              : hostname.includes("incompatible")
                ? "incompatible_version"
                : undefined;
      if (scenario) return { kind: "error", error: scenario };
      const server: RemoteServerInfo = {
        version: "0.8.35-demo",
        apiVersion: 2,
        state: hostname.includes("starting") ? "starting" : "ready",
        devices: [
          {
            id: "cuda:0",
            name: "NVIDIA GeForce RTX 3060 Ti · demo",
            backend: "cuda",
            available: true,
          },
        ],
        models: [
          {
            id: "large_turbo",
            label: "Whisper large-v3-turbo · demo",
            installed: true,
            state: "ready",
            languages: ["ru", "en"],
            device: "cuda:0",
          },
          {
            id: "giga_am:v3_e2e_rnnt_int8",
            label: "GigaAM v3 · demo",
            installed: true,
            state: "unloaded",
            languages: ["ru"],
            device: "cpu",
          },
        ],
      };
      if (
        input.model &&
        !server.models.some(
          (model) => model.id === input.model && model.installed,
        )
      )
        return { kind: "error", error: "model_not_found" };
      return { kind: "ok", server };
    },
    async saveToken(token) {
      guard();
      if (!/^[A-Za-z0-9._~+/=-]{1,4096}$/.test(token))
        throw new Error(message("recognitionSource.invalidToken"));
      await wait();
      guard();
      hasToken = true;
    },
    async clearToken() {
      guard();
      await wait();
      guard();
      hasToken = false;
    },
  };
}
