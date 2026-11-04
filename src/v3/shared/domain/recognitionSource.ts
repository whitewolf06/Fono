/** Public preferences only. Tokens belong to the transient form and the OS vault. */
export interface RecognitionSourceSettings {
  source: "local" | "fono_server";
  url: string;
  displayName: string;
  model: string;
  caPem: string;
  fallback: "none" | "ask" | "local";
  fallbackModel: string;
}
export function defaultRecognitionSource(): RecognitionSourceSettings {
  return {
    source: "local",
    url: "",
    displayName: "",
    model: "",
    caPem: "",
    fallback: "none",
    fallbackModel: "",
  };
}
export type RemoteConnectionError =
  | "invalid_endpoint"
  | "not_configured"
  | "network"
  | "timeout"
  | "unauthorized"
  | "incompatible_version"
  | "model_not_found"
  | "invalid_response"
  | "busy"
  | "server_failed";
export interface RemoteServerInfo {
  version: string;
  apiVersion: number;
  state: "ready" | "starting" | "busy" | "failed" | "stopping";
  devices: { id: string; name: string; backend: string; available: boolean }[];
  models: {
    id: string;
    label: string;
    installed: boolean;
    state: string;
    languages: string[];
    device: string;
  }[];
}
export type RemoteConnectionResult =
  | { kind: "ok"; server: RemoteServerInfo }
  | { kind: "error"; error: RemoteConnectionError };
export interface RemoteConnectionInput {
  url: string;
  token?: string;
  caPem?: string;
  model?: string;
}
export interface RecognitionSourcePort {
  status(): Promise<{ hasToken: boolean }>;
  test(input: RemoteConnectionInput): Promise<RemoteConnectionResult>;
  saveToken(token: string): Promise<void>;
  clearToken(): Promise<void>;
}

export function validRemoteEndpoint(value: string): boolean {
  try {
    const url = new URL(value);
    const loopback =
      url.hostname === "localhost" ||
      url.hostname === "[::1]" ||
      /^127\.\d+\.\d+\.\d+$/.test(url.hostname);
    return (
      value.length <= 2048 &&
      !url.username &&
      !url.password &&
      url.pathname === "/" &&
      !url.search &&
      !url.hash &&
      (url.protocol === "https:" || (url.protocol === "http:" && loopback))
    );
  } catch {
    return false;
  }
}
