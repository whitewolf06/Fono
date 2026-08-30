import {
  ipc,
  onLocalTranscriptionServiceChanged,
  onSettingsChange,
} from "@/lib/ipc";
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
    clearHistory: () => ipc.clearLocalTranscriptionHistory(),
    copyText: (text) => ipc.copyDictationText(text),
    subscribe(listener) {
      let active = true;
      let stopService: (() => void) | undefined;
      let stopSettings: (() => void) | undefined;

      const subscribe = async () => {
        const [nextStopService, nextStopSettings] = await Promise.all([
          onLocalTranscriptionServiceChanged(listener),
          onSettingsChange(listener),
        ]);
        if (active) {
          stopService = nextStopService;
          stopSettings = nextStopSettings;
        } else {
          nextStopService();
          nextStopSettings();
        }
      };
      void subscribe();

      return () => {
        active = false;
        stopService?.();
        stopSettings?.();
      };
    },
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
    history: {
      jobs: service.history.jobs.map(toServiceJob),
      completed: service.history.completed,
      failed: service.history.failed,
      cancelled: service.history.cancelled,
      totalAudioSeconds: service.history.total_audio_seconds,
      totalTranscribeSeconds: service.history.total_transcribe_seconds,
    },
  };
}

function toServiceJob(job: LocalTranscriptionJob): ServiceJob {
  return {
    id: job.id,
    requestedLanguage: job.requested_language,
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
