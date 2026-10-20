import type {
  Settings,
  SttModelInfo,
  DictationHistoryEntry,
  LocalTranscriptionJob,
} from "./ipcTypes";
import type {
  Preferences,
  ConnectionProfile,
  Dictation,
  SpeechModel,
  ServiceJob,
} from "../../domain/contracts";
import { defaults } from "../../../features/preferences/domain/preferences";
import { measuredModel } from "../../domain/sttMeasurements";
import { availableDictationMode } from "../../domain/dictationMode";
import { availableWakeEnabled } from "../../domain/wakeAvailability";
import { availableGpuModelResidency } from "../../domain/gpuMemory";
import { availableTrigger } from "../../domain/triggers";
import { historyMetadataFromNative } from "./historyMetadata";
import {
  modelKey,
  compatibleModelPreferences,
  validateModelPreferences,
} from "../../domain/speechModels";
import {
  emptyProcessingPrompts,
  promptPresets,
} from "../../../features/preferences/domain/processingPrompts";
import {
  validateWakePhrase,
  wakePhraseLanguage,
} from "../../../features/preferences/domain/wakePhrase";
export function preferencesFromNative(
  s: Settings,
  models: SttModelInfo[],
): Preferences {
  const catalogModel = s.whisper_model_path
    ? models.find((m) => m.local_path === s.whisper_model_path)
    : undefined;
  return {
    ...defaults,
    pretranscriptionEnabled: s.pretranscription_enabled === true,
    dictationMode: availableDictationMode(s.dictation_mode),
    microphone: s.audio_device_id || "system",
    model:
      (s.stt_model
        ? s.stt_model.id.variant === "custom" &&
          s.stt_model.id.engine === "whisper"
          ? s.stt_model.local_path
          : modelKey(s.stt_model.id)
        : undefined) ||
      (catalogModel ? modelKey(catalogModel.id) : undefined) ||
      s.whisper_model_path ||
      "",
    language: s.language,
    acceleration: s.acceleration,
    whisperDecoding: s.whisper_decoding === "accurate" ? "accurate" : "fast",
    whisperThreads:
      Number.isInteger(s.whisper_threads) &&
      Number(s.whisper_threads) >= 1 &&
      Number(s.whisper_threads) <= 8
        ? s.whisper_threads!
        : null,
    gpuModelResidency: availableGpuModelResidency(s.gpu_model_residency),
    dictionaryEnabled: s.personal_dictionary_enabled ?? false,
    dictionaryLlmEnabled: s.personal_dictionary_llm_enabled ?? false,
    dictionaryEntries: (s.personal_dictionary_entries ?? []).map((entry) => ({
      written: entry.written,
      spoken: [...entry.spoken],
    })),
    hotkey: s.hotkey
      .split("+")
      .map((x) => x.trim())
      .join(" + "),
    hotkeyMode: s.hotkey_mode ?? "hold",
    dictationTrigger: availableTrigger(s.dictation_trigger),
    selectionCaptureEnabled: s.selection_capture_enabled ?? false,
    selectionHotkey: (s.selection_hotkey || "Ctrl+Alt+Space")
      .split("+")
      .map((x) => x.trim())
      .join(" + "),
    selectionClipboardFallback: s.selection_clipboard_fallback ?? false,
    selectionAutoInsert: s.selection_auto_insert ?? false,
    selectionHistoryEnabled: s.selection_history_enabled ?? false,
    commandHotkey: s.command_hotkey
      .split("+")
      .map((x) => x.trim())
      .join(" + "),
    wakeEnabled: availableWakeEnabled(s.wake_word_enabled),
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
    processingTranslationEnabled: s.processing_translation_enabled ?? true,
    processingPrompts: Object.fromEntries(
      promptPresets.map((preset) => [
        preset,
        s.processing_prompts?.[preset]
          ? {
              useCustom: s.processing_prompts[preset].use_custom,
              customPrompt: s.processing_prompts[preset].custom_prompt,
            }
          : preset === "clean" && s.clean_prompt?.trim()
            ? { useCustom: true, customPrompt: s.clean_prompt }
            : emptyProcessingPrompts()[preset],
      ]),
    ) as Preferences["processingPrompts"],
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
    overlayQuickProcessing: s.overlay_quick_processing ?? true,
    overlayScale: Math.round(s.overlay_scale * 100),
    overlayOpacity: Math.round(s.overlay_opacity * 100),
    overlayPosition: "custom",
    historyEnabled: s.history_enabled,
    historyRetentionDays: s.history_retention_days ?? 365,
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
  models: SttModelInfo[],
): Settings {
  const next = structuredClone(s);
  next.dictation_mode = availableDictationMode(patch.dictationMode);
  let p = { ...preferencesFromNative(s, models), ...patch };
  const catalog = modelsFromNative(models);
  const selected = catalog.find((m) => m.id === p.model);
  if ("model" in patch)
    p = { ...p, ...compatibleModelPreferences(selected, p) };
  const has = (key: keyof Preferences) => key in patch;
  const pairs = {
    pretranscriptionEnabled: "pretranscription_enabled",
    dictationTrigger: "dictation_trigger",
    selectionCaptureEnabled: "selection_capture_enabled",
    selectionClipboardFallback: "selection_clipboard_fallback",
    selectionAutoInsert: "selection_auto_insert",
    selectionHistoryEnabled: "selection_history_enabled",
    microphone: "audio_device_id",
    language: "language",
    acceleration: "acceleration",
    whisperDecoding: "whisper_decoding",
    whisperThreads: "whisper_threads",
    gpuModelResidency: "gpu_model_residency",
    dictionaryEnabled: "personal_dictionary_enabled",
    dictionaryLlmEnabled: "personal_dictionary_llm_enabled",
    wakeEnabled: "wake_word_enabled",
    wakePhrase: "wake_word",
    silenceMs: "wake_dictation_silence_ms",
    wakeThreshold: "wake_word_threshold",
    speechThreshold: "wake_dictation_speech_threshold",
    autostart: "autostart",
    insertion: "injection_mode",
    overlayEnabled: "overlay_enabled",
    overlayCompact: "overlay_mini_mode",
    overlayQuickProcessing: "overlay_quick_processing",
    historyEnabled: "history_enabled",
    historyRetentionDays: "history_retention_days",
    trainerEnabled: "speech_trainer_enabled",
    analyticsConsent: "analytics_enabled",
    retentionDays: "analytics_retention_days",
    verboseLogging: "verbose_logging",
  } as const;
  for (const [key, nativeKey] of Object.entries(pairs))
    if (has(key as keyof Preferences))
      Object.assign(next, { [nativeKey]: p[key as keyof Preferences] });
  next.wake_word_enabled = availableWakeEnabled(next.wake_word_enabled);
  if (has("microphone"))
    next.audio_device_id = p.microphone === "system" ? null : p.microphone;
  if (has("dictionaryEntries"))
    next.personal_dictionary_entries = p.dictionaryEntries.map((entry) => ({
      written: entry.written,
      spoken: [...entry.spoken],
    }));
  if (has("model")) {
    const model = models.find(
      (m) => modelKey(m.id) === p.model && m.local_path,
    );
    if (!model && p.model !== s.whisper_model_path)
      throw new Error("Выберите установленную модель");
    if (model) {
      next.stt_model = { id: { ...model.id }, local_path: model.local_path! };
      next.whisper_model_path =
        model.id.engine === "whisper" ? model.local_path : null;
      next.language = p.language;
      next.acceleration = p.acceleration as Settings["acceleration"];
    }
  }
  if (selected) validateModelPreferences(selected, p);
  if (has("hotkey")) next.hotkey = p.hotkey.replaceAll(" ", "");
  if (has("hotkeyMode")) next.hotkey_mode = p.hotkeyMode;
  if (has("selectionHotkey"))
    next.selection_hotkey = p.selectionHotkey.replaceAll(" ", "");
  if (has("processingTrigger")) next.processing_workflow = p.processingTrigger;
  if (has("processingMode")) next.processing_preset = p.processingMode;
  if (has("processingTranslation"))
    next.processing_target_language =
      p.processingTranslation === "none" ? null : p.processingTranslation;
  if (has("processingTranslationEnabled"))
    next.processing_translation_enabled = p.processingTranslationEnabled;
  if (has("processingPrompts")) {
    next.processing_prompts = Object.fromEntries(
      promptPresets.map((preset) => [
        preset,
        {
          use_custom: p.processingPrompts[preset].useCustom,
          custom_prompt: p.processingPrompts[preset].customPrompt,
        },
      ]),
    ) as NonNullable<Settings["processing_prompts"]>;
    next.clean_prompt = null;
  }
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
  if (has("instruction") && !has("processingPrompts"))
    next.clean_prompt = p.instruction || null;
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
export function modelsFromNative(models: SttModelInfo[]): SpeechModel[] {
  return models.map((m) => ({
    id: modelKey(m.id),
    engine: m.id.engine,
    benchmark: measuredModel(
      modelKey(m.id),
      m.files?.length
        ? m.files
        : m.sha256
          ? [{ filename: m.filename, bytes: m.bytes, sha256: m.sha256 }]
          : [],
    ),
    name:
      m.id.engine === "whisper"
        ? whisperModelName(m.id.variant)
        : m.id.engine === "giga_am"
          ? "GigaAM v3 · e2e RNNT INT8"
          : "Parakeet v3 · TDT 0.6B INT8",
    license: m.license,
    licenseUrl: m.license_url,
    languages: m.languages,
    multilingual: m.multilingual,
    punctuation: m.punctuation,
    languageSelection: m.capabilities.language_selection,
    decodingOptions: m.capabilities.decoding_options,
    accelerations: m.capabilities.accelerations,
    description:
      m.id.engine === "whisper" && /_q[58]_/.test(m.id.variant)
        ? "Квантованная модель. Качество зависит от записи."
        : m.filename,
    size: m.bytes
      ? Math.round(m.bytes / 1048576) + " МБ"
      : "По размеру загрузки",
    status: m.local_path ? "installed" : "available",
    progress: m.local_path ? 100 : 0,
  }));
}
function whisperModelName(variant: string): string {
  const [base, quantization] = variant.split(/_(?=q[58]_)/);
  const title =
    base === "large_turbo"
      ? "Large v3 Turbo"
      : base[0]?.toUpperCase() + base.slice(1);
  return `Whisper ${title}${quantization ? " · " + quantization.toUpperCase() : ""}`;
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
    analyticsIncluded: e.analytics_included !== false,
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
