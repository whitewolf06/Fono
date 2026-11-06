import type { RecognitionSourceSettings } from "../../domain/recognitionSource";
import {
  defaultRecognitionSource,
  validRemoteEndpoint,
  validRemoteSsh,
} from "../../domain/recognitionSource";
import { modelId, modelKey } from "../../domain/speechModels";
import { message } from "../../domain/messages";
import type { Settings } from "./ipcTypes";

export function recognitionSourceFromNative(
  settings: Settings,
): RecognitionSourceSettings {
  const remote = settings.remote_stt;
  const fallback = remote?.fallback;
  return {
    ...defaultRecognitionSource(),
    source: settings.stt_source === "fono_server" ? "fono_server" : "local",
    url: remote?.url ?? "",
    displayName: remote?.display_name ?? "",
    model: remote?.model ? modelKey(remote.model) : "",
    caPem: remote?.ca_pem ?? "",
    ...(remote?.ssh
      ? {
          ssh: {
            host: remote.ssh.host,
            port: remote.ssh.port,
            user: remote.ssh.user,
            identityFile: remote.ssh.identity_file ?? "",
            serverPort: remote.ssh.server_port,
            hostKey: remote.ssh.host_key,
          },
        }
      : {}),
    fallback:
      typeof fallback === "object" && fallback?.local
        ? "local"
        : fallback === "ask"
          ? "ask"
          : "none",
    fallbackModel:
      typeof fallback === "object" && fallback?.local
        ? fallback.local.engine === "whisper" &&
          fallback.local.variant === "custom" &&
          settings.stt_model?.id.variant === "custom"
          ? settings.stt_model.local_path
          : modelKey(fallback.local)
        : "",
  };
}
export function applyRecognitionSource(
  settings: Settings,
  value: RecognitionSourceSettings,
): void {
  if (
    !["local", "fono_server"].includes(value.source) ||
    value.displayName.length > 80 ||
    value.caPem.length > 256 * 1024 ||
    !["none", "ask", "local"].includes(value.fallback)
  )
    throw new Error(message("recognitionSource.invalidSettings"));
  settings.stt_source = value.source;
  if (value.source === "local") return;
  if (
    !validRemoteEndpoint(value.url) ||
    (value.ssh && !validRemoteSsh(value.ssh)) ||
    !value.model ||
    (value.fallback === "local" && !value.fallbackModel)
  )
    throw new Error(message("recognitionSource.invalidSettings"));
  settings.remote_stt = {
    url: value.url.trim(),
    display_name: value.displayName.trim() || null,
    model: modelId(value.model),
    ca_pem: value.caPem.trim() || null,
    ...(value.ssh
      ? {
          ssh: {
            host: value.ssh.host,
            port: value.ssh.port,
            user: value.ssh.user,
            identity_file: value.ssh.identityFile || null,
            server_port: value.ssh.serverPort,
            host_key: value.ssh.hostKey,
          },
        }
      : {}),
    fallback:
      value.fallback === "local"
        ? {
            local:
              settings.stt_model?.local_path === value.fallbackModel
                ? { ...settings.stt_model.id }
                : modelId(value.fallbackModel),
          }
        : value.fallback,
  };
}
