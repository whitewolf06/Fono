import { ipc } from "@/lib/ipc";
import type {
  LocalTranscriptionJob,
  LocalTranscriptionServiceSnapshot,
  Settings,
} from "@/lib/types";
import {
  whisperModelName,
  whisperModelSizeFromPath,
} from "@/v2/shared/domain/whisperModels";
import type { ServiceRuntime } from "../application/serviceRuntime";
import type { ServiceJob, ServiceSnapshot } from "../domain/serviceMonitor";

export function createTauriServiceRuntime(): ServiceRuntime {
  return {
    async getSnapshot() {
      const [service, settings] = await Promise.all([
        ipc.getLocalTranscriptionServiceSnapshot(),
        ipc.getSettings(),
      ]);
      return toServiceSnapshot(service, settings);
    },
    async cancelJob(id) {
      await ipc.cancelLocalTranscriptionJob(id);
    },
    copyText: (text) => ipc.copyDictationText(text),
  };
}

function toServiceSnapshot(
  service: LocalTranscriptionServiceSnapshot,
  settings: Settings,
): ServiceSnapshot {
  return {
    address: service.address,
    protocolVersion: service.protocol_version,
    model: modelLabel(settings.whisper_model_path),
    acceleration:
      settings.acceleration === "auto"
        ? "Авто"
        : settings.acceleration.toUpperCase(),
    queue: {
      ...service.queue,
      jobs: service.queue.jobs.map(toServiceJob),
    },
  };
}

function toServiceJob(job: LocalTranscriptionJob): ServiceJob {
  return {
    id: job.id,
    state: job.state,
    createdAtMs: job.created_at_ms,
    startedAtMs: job.started_at_ms,
    finishedAtMs: job.finished_at_ms,
    result: job.result && {
      text: job.result.text,
      detectedLanguage: job.result.detected_language,
      audioSeconds: job.result.audio_seconds,
      transcribeSeconds: job.result.transcribe_seconds,
      model: job.result.model,
      backend: job.result.backend,
    },
    error: job.error?.message ?? null,
  };
}

function modelLabel(path: string | null): string {
  if (!path) return "Модель не выбрана";
  const size = whisperModelSizeFromPath(path);
  return size ? `Whisper ${whisperModelName(size)}` : "Whisper";
}
