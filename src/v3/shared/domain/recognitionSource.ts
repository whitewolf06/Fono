/** Native preferences only. Identity paths and host pins never enter browser storage. */
export interface RemoteSshSettings {
  host: string;
  port: number;
  user: string;
  identityFile: string;
  serverPort: number;
  hostKey: string;
}
export function defaultRemoteSsh(): RemoteSshSettings {
  return {
    host: "",
    port: 22,
    user: "",
    identityFile: "",
    serverPort: 17832,
    hostKey: "",
  };
}
export function validSshHost(host: string, port: number): boolean {
  if (
    typeof host !== "string" ||
    !host ||
    host.length > 253 ||
    !Number.isInteger(port) ||
    port < 1 ||
    port > 65535
  )
    return false;
  if (host.includes(":")) {
    try {
      return (
        /^[0-9a-fA-F:]+$/.test(host) &&
        new URL(`http://[${host}]/`).hostname.startsWith("[")
      );
    } catch {
      return false;
    }
  }
  return host
    .split(".")
    .every((label) =>
      /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/.test(label),
    );
}
export function validRemoteSsh(ssh: RemoteSshSettings): boolean {
  return (
    !!ssh &&
    validSshHost(ssh.host, ssh.port) &&
    typeof ssh.user === "string" &&
    /^[A-Za-z0-9_][A-Za-z0-9_.-]{0,63}$/.test(ssh.user) &&
    Number.isInteger(ssh.serverPort) &&
    ssh.serverPort >= 1 &&
    ssh.serverPort <= 65535 &&
    typeof ssh.identityFile === "string" &&
    ssh.identityFile.length <= 4096 &&
    ![...ssh.identityFile].some(
      (character) =>
        character.charCodeAt(0) < 32 || character.charCodeAt(0) === 127,
    ) &&
    /^ssh-ed25519 [A-Za-z0-9+/]{68}$/.test(ssh.hostKey)
  );
}
/** Public preferences only. Tokens belong to the transient form and the OS vault. */
export interface RecognitionSourceSettings {
  source: "local" | "fono_server";
  url: string;
  displayName: string;
  model: string;
  caPem: string;
  fallback: "none" | "ask" | "local";
  fallbackModel: string;
  ssh?: RemoteSshSettings;
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
  | "unsupported_capabilities"
  | "model_not_found"
  | "invalid_response"
  | "busy"
  | "ssh_invalid_configuration"
  | "ssh_client_missing"
  | "ssh_identity_missing"
  | "ssh_host_key_required"
  | "ssh_connection_failed"
  | "ssh_busy"
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
  ssh?: RemoteSshSettings;
}
export type SshHostKeyResult =
  | { kind: "ok"; hostKey: string; fingerprint: string }
  | { kind: "error"; error: RemoteConnectionError };
export interface RecognitionSourcePort {
  status(): Promise<{ hasToken: boolean }>;
  test(input: RemoteConnectionInput): Promise<RemoteConnectionResult>;
  saveToken(token: string): Promise<void>;
  clearToken(): Promise<void>;
  probeSshHost(input: {
    host: string;
    port: number;
  }): Promise<SshHostKeyResult>;
  pickIdentityFile(): Promise<string | null>;
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
