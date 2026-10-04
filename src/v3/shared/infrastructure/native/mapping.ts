import type {
  Settings,
  WhisperModelInfo,
  DictationHistoryEntry,
  LocalTranscriptionJob,
} from "../../../../lib/types";
import type {
  Preferences,
  ConnectionProfile,
  Dictation,
  SpeechModel,
  ServiceJob,
} from "../../domain/contracts";
import { defaults } from "../../../features/preferences/domain/preferences";
import { availableDictationMode } from "../../domain/dictationMode";
import { historyMetadataFromNative } from "./historyMetadata";
import {
  validateWakePhrase,
  wakePhraseLanguage,
} from "../../../features/preferences/domain/wakePhrase";
export function preferencesFromNative(
  s: Settings,
  models: WhisperModelInfo[],
): Preferences {
  return {
    ...defaults,
    dictationMode: availableDictationMode(s.dictation_mode),
    microphone: s.audio_device_id || "system",
    model:
      models.find((m) => m.local_path === s.whisper_model_path)?.size ||
      s.whisper_model_path ||
      "",
    language: s.language,
    acceleration: s.acceleration,
    dictionaryEnabled: s.personal_dictionary_enabled ?? false,
    dictionaryEntries: (s.personal_dictionary_entries ?? []).map((entry) => ({
      written: entry.written,
      spoken: [...entry.spoken],
    })),
    hotkey: s.hotkey
      .split("+")
      .map((x) => x.trim())
      .join(" + "),
    hotkeyMode: s.hotkey_mode ?? "hold",
    commandHotkey: s.command_hotkey
      .split("+")
      .map((x) => x.trim())
      .join(" + "),
    wakeEnabled: s.wake_word_enabled,
    wakePhrase: s.wake_word,
    wakeLanguage:
      s.wake_backend === "sherpa_streaming_ru"
        ? "ru"
        : s.wake_backend === "sherpa_streaming_en"
          ? "en"
          : wakePhraseLanguage(s.wake_word),
    silenceMs: s.wake_dictation_silence_ms,
    wakeThreshold: s.wake_word_threshold,
    speechThreshold: s.wake_dictation_speech_threshold,
    processingEnabled: s.ai_mode !== "off",
    processingMode:
      s.processing_preset ?? (s.ai_mode === "format" ? "format" : "clean"),
    processingTrigger: s.processing_workflow ?? "automatic",
    processingTranslation: s.processing_target_language ?? "none",
    profile: s.text_correction_llm.profile_id || "",
    processingModel:
      s.text_correction_llm.model ||
      s.llm_profiles.find((p) => p.id === s.text_correction_llm.profile_id)
        ?.model ||
      "",
    instruction: s.clean_prompt || "",
    autostart: s.autostart,
    insertion: s.injection_mode,
    overlayEnabled: s.overlay_enabled,
    overlayCompact: s.overlay_mini_mode,
    overlayScale: Math.round(s.overlay_scale * 100),
    overlayOpacity: Math.round(s.overlay_opacity * 100),
    overlayPosition: "custom",
    historyEnabled: s.history_enabled,
    trainerEnabled: s.speech_trainer_enabled && s.analytics_enabled,
    analyticsConsent: s.analytics_enabled,
    retentionDays: s.analytics_retention_days,
    trainerAiEnabled: s.speech_analysis_llm.enabled,
    trainerProfile: s.speech_analysis_llm.profile_id || "",
    trainerModel: s.speech_analysis_llm.model || "",
    trainerScope: s.speech_analysis_llm.data_scope,
    cloudConsent: s.speech_analysis_llm.cloud_consent,
    serviceEnabled: s.service_enabled,
    verboseLogging: s.verbose_logging,
  };
}
export function applyPreferences(
  s: Settings,
  patch: Partial<Preferences>,
  models: WhisperModelInfo[],
): Settings {
  const next = structuredClone(s);
  next.dictation_mode = availableDictationMode(patch.dictationMode);
  const p = { ...preferencesFromNative(s, models), ...patch };
  const has = (key: keyof Preferences) => key in patch;
  const pairs = {
    microphone: "audio_device_id",
    language: "language",
    acceleration: "acceleration",
    dictionaryEnabled: "personal_dictionary_enabled",
    wakeEnabled: "wake_word_enabled",
    wakePhrase: "wake_word",
    silenceMs: "wake_dictation_silence_ms",
    wakeThreshold: "wake_word_threshold",
    speechThreshold: "wake_dictation_speech_threshold",
    autostart: "autostart",
    insertion: "injection_mode",
    overlayEnabled: "overlay_enabled",
    overlayCompact: "overlay_mini_mode",
    historyEnabled: "history_enabled",
    trainerEnabled: "speech_trainer_enabled",
    analyticsConsent: "analytics_enabled",
    retentionDays: "analytics_retention_days",
    verboseLogging: "verbose_logging",
  } as const;
  for (const [key, nativeKey] of Object.entries(pairs))
    if (has(key as keyof Preferences))
      Object.assign(next, { [nativeKey]: p[key as keyof Preferences] });
  if (has("microphone"))
    next.audio_device_id = p.microphone === "system" ? null : p.microphone;
  if (has("dictionaryEntries"))
    next.personal_dictionary_entries = p.dictionaryEntries.map((entry) => ({
      written: entry.written,
      spoken: [...entry.spoken],
    }));
  if (has("model")) {
    const model = models.find((m) => m.size === p.model && m.local_path);
    if (!model && p.model !== s.whisper_model_path)
      throw new Error("Выберите установленную модель");
    next.whisper_model_path = model?.local_path || s.whisper_model_path;
  }
  if (has("hotkey")) next.hotkey = p.hotkey.replaceAll(" ", "");
  if (has("hotkeyMode")) next.hotkey_mode = p.hotkeyMode;
  if (has("processingTrigger")) next.processing_workflow = p.processingTrigger;
  if (has("processingMode")) next.processing_preset = p.processingMode;
  if (has("processingTranslation"))
    next.processing_target_language =
      p.processingTranslation === "none" ? null : p.processingTranslation;
  if (has("wakeLanguage") || has("wakePhrase")) {
    const error = validateWakePhrase(p.wakePhrase, p.wakeLanguage);
    if (error) throw new Error(error);
    next.wake_backend =
      p.wakeLanguage === "ru" ? "sherpa_streaming_ru" : "sherpa_streaming_en";
  }
  if (has("commandHotkey"))
    next.command_hotkey = p.commandHotkey.replaceAll(" ", "");
  if (has("processingEnabled") || has("processingMode"))
    next.ai_mode = p.processingEnabled
      ? p.processingMode === "clean"
        ? "clean"
        : "format"
      : "off";
  if (has("profile")) next.text_correction_llm.profile_id = p.profile || null;
  if (has("processingModel"))
    next.text_correction_llm.model = p.processingModel || null;
  if (has("instruction")) next.clean_prompt = p.instruction || null;
  if (has("overlayScale")) next.overlay_scale = p.overlayScale / 100;
  if (has("overlayOpacity")) next.overlay_opacity = p.overlayOpacity / 100;
  if (has("trainerAiEnabled"))
    next.speech_analysis_llm.enabled = p.trainerAiEnabled;
  if (has("trainerProfile"))
    next.speech_analysis_llm.profile_id = p.trainerProfile || null;
  if (has("trainerModel"))
    next.speech_analysis_llm.model = p.trainerModel || null;
  if (has("trainerScope")) next.speech_analysis_llm.data_scope = p.trainerScope;
  if (has("cloudConsent"))
    next.speech_analysis_llm.cloud_consent = p.cloudConsent;
  if (!next.analytics_enabled) {
    next.speech_trainer_enabled = false;
    next.speech_analysis_llm.enabled = false;
  }
  if (
    !next.speech_analysis_llm.cloud_consent &&
    next.llm_profiles.find((x) => x.id === next.speech_analysis_llm.profile_id)
      ?.connection === "cloud"
  )
    next.speech_analysis_llm.enabled = false;
  return next;
}
export function profilesFromNative(s: Settings): ConnectionProfile[] {
  return s.llm_profiles.map((p) => ({
    id: p.id,
    name: p.name,
    provider: p.provider,
    location: p.connection,
    url: p.base_url,
    model: p.model || "",
    hasApiKey: p.has_api_key,
  }));
}
export function modelsFromNative(models: WhisperModelInfo[]): SpeechModel[] {
  return models.map((m) => ({
    id: m.size,
    name: "Whisper " + m.size.replaceAll("_", " "),
    description: m.filename,
    size: m.bytes
      ? Math.round(m.bytes / 1048576) + " МБ"
      : "По размеру загрузки",
    status: m.local_path ? "installed" : "available",
    progress: m.local_path ? 100 : 0,
  }));
}
export function historyFromNative(e: DictationHistoryEntry): Dictation {
  const kinds = [
    "filler",
    "repetition",
    "self_correction",
    "unfinished",
  ] as const;
  const metadata = historyMetadataFromNative(e);
  return {
    id: e.id,
    text: e.text,
    original: e.original_text || undefined,
    createdAt: e.created_at,
    duration:
      metadata?.recordingDurationMs != null
        ? metadata.recordingDurationMs / 1000
        : 0,
    metadata,
    title: e.text.slice(0, 64),
    findings: kinds.map((kind, i) => ({
      title: [
        "Слова-паразиты",
        "Повторы",
        "Самоисправления",
        "Незавершённые фразы",
      ][i],
      count: e.analysis
        ? [
            e.analysis.filler_count,
            e.analysis.repetition_count,
            e.analysis.self_correction_count,
            e.analysis.unfinished_count ?? 0,
          ][i]
        : 0,
      example:
        e.analysis?.findings
          .filter((f) => f.kind === kind)
          .map((f) => f.fragment)
          .join(", ") || "",
      advice: "Делайте короткую паузу между мыслями.",
    })),
    analysisReady: e.analysis_status === "ready",
  };
}
export function jobFromNative(j: LocalTranscriptionJob): ServiceJob {
  return {
    id: j.id,
    name: "Задача " + j.id.slice(0, 8),
    state:
      j.state === "completed"
        ? "done"
        : j.state === "failed"
          ? "error"
          : j.state === "queued"
            ? "queued"
            : j.state === "cancelled"
              ? "cancelled"
              : "running",
    text: j.result?.text,
    error: j.error?.message,
    seconds: Math.round(j.result?.audio_seconds || 0),
  };
}
