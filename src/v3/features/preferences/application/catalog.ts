import { message as uiMessage } from "../../../shared/domain/messages";
import type { Section } from "../../../shared/domain/contracts";
import type { AppIconName } from "../../../shared/domain/icons";
import { LIVE_DICTATION_ENABLED } from "../../../shared/domain/dictationMode";
export const sections: {
  id: Section;
  title: string;
  icon: AppIconName;
  description: string;
}[] = [
  {
    id: "general",
    get title() {
      return uiMessage("ui.d40de76d56");
    },
    icon: "sliders",
    get description() {
      return uiMessage("ui.716ba9e655");
    },
  },
  {
    id: "audio",
    get title() {
      return uiMessage("ui.606a246350");
    },
    icon: "microphone",
    get description() {
      return uiMessage("ui.a6a2ebf0e2");
    },
  },
  {
    id: "activation",
    get title() {
      return uiMessage("ui.902d6f27ec");
    },
    icon: "keyboard",
    get description() {
      return uiMessage("ui.f75f85e366");
    },
  },
  {
    id: "processing",
    get title() {
      return uiMessage("ui.84ed05650c");
    },
    icon: "sparkle",
    get description() {
      return uiMessage("ui.59015bef60");
    },
  },
  {
    id: "overlay",
    get title() {
      return uiMessage("ui.f4c1856607");
    },
    icon: "panel",
    get description() {
      return uiMessage("ui.e6d59c60a4");
    },
  },
  {
    id: "privacy",
    get title() {
      return uiMessage("ui.c48cd755f9");
    },
    icon: "shield",
    get description() {
      return uiMessage("ui.d2490aa6ca");
    },
  },
  {
    id: "diagnostics",
    get title() {
      return uiMessage("ui.da4da4e352");
    },
    icon: "activity",
    get description() {
      return uiMessage("ui.6f1713f7e8");
    },
  },
];
export const searchIndex: {
  label: string;
  section: Section;
  field: string;
  keywords: string;
}[] = [
  {
    get label() {
      return uiMessage("recordingSounds.label");
    },
    section: "activation",
    field: "recordingSoundsEnabled",
    keywords:
      "звуки сигналы громкость начало остановка ошибка sounds cues volume start stop error",
  },
  {
    get label() {
      return uiMessage("ui.9089a73112");
    },
    section: "general",
    field: "uiLanguage",
    keywords:
      "язык интерфейса русский английский language interface locale Russian English",
  },
  {
    get label() {
      return uiMessage("autoStop.label");
    },
    section: "activation",
    field: "toggleAutoStopMs",
    keywords: "автостоп тишина пауза toggle auto stop silence VAD",
  },
  {
    get label() {
      return uiMessage("ui.296d198409");
    },
    section: "activation",
    field: "selectionCaptureEnabled",
    get keywords() {
      return uiMessage("ui.c5d97dda92");
    },
  },
  {
    get label() {
      return uiMessage("ui.f038415295");
    },
    section: "audio",
    field: "microphone",
    get keywords() {
      return uiMessage("ui.fc3c4c5aac");
    },
  },
  {
    get label() {
      return uiMessage("settings.dictionary");
    },
    section: "audio",
    field: "dictionary",
    get keywords() {
      return uiMessage("ui.7c37a64586");
    },
  },
  {
    get label() {
      return uiMessage("ui.8b9baf7846");
    },
    section: "audio",
    field: "model",
    get keywords() {
      return uiMessage("ui.545800ad64");
    },
  },
  {
    get label() {
      return uiMessage("ui.4cfc792253");
    },
    section: "audio",
    field: "gpu-memory",
    get keywords() {
      return uiMessage("ui.c80fd82b48");
    },
  },
  {
    get label() {
      return uiMessage("ui.18fb8ccf7e");
    },
    section: "activation",
    field: "hotkey",
    get keywords() {
      return uiMessage("ui.3b1b55fba2");
    },
  },
  {
    get label() {
      return uiMessage("ui.14e1110db4");
    },
    section: "activation",
    field: "hotkeyMode",
    get keywords() {
      return uiMessage("ui.e7f878223e");
    },
  },
  {
    get label() {
      return uiMessage("ui.c87d95102c");
    },
    section: "activation",
    field: "wakePhrase",
    get keywords() {
      return uiMessage("ui.d62d3c027a");
    },
  },
  {
    get label() {
      return uiMessage("ui.1caa95b395");
    },
    section: "activation",
    field: "silenceMs",
    get keywords() {
      return uiMessage("ui.a64f15b843");
    },
  },
  {
    get label() {
      return uiMessage("ui.7744360ac2");
    },
    section: "activation",
    field: "advanced",
    get keywords() {
      return uiMessage("ui.fbb9eb0ee5");
    },
  },
  {
    get label() {
      return uiMessage("ui.d4a26d3502");
    },
    section: "processing",
    field: "processingMode",
    get keywords() {
      return uiMessage("ui.2d493eec09");
    },
  },
  {
    get label() {
      return uiMessage("ui.1493851f52");
    },
    section: "processing",
    field: "processingTrigger",
    get keywords() {
      return uiMessage("ui.ab387b21d0");
    },
  },
  {
    get label() {
      return uiMessage("ui.6dbccf713c");
    },
    section: "processing",
    field: "processingTranslation",
    get keywords() {
      return uiMessage("ui.0b2238a56d");
    },
  },
  {
    get label() {
      return uiMessage("ui.976cfb82ff");
    },
    section: "processing",
    field: "profile",
    get keywords() {
      return uiMessage("ui.175f0f9475");
    },
  },
  {
    get label() {
      return uiMessage("settings.prompts");
    },
    section: "processing",
    field: "processingPrompts",
    get keywords() {
      return uiMessage("ui.db0f8b44c4");
    },
  },
  {
    get label() {
      return uiMessage("ui.91c097aca3");
    },
    section: "privacy",
    field: "trainerModel",
    get keywords() {
      return uiMessage("ui.8694023a12");
    },
  },
  {
    get label() {
      return uiMessage("ui.940d898a7a");
    },
    section: "overlay",
    field: "overlayScale",
    get keywords() {
      return uiMessage("ui.9ef902d412");
    },
  },
  {
    get label() {
      return uiMessage("ui.b5270b927f");
    },
    section: "overlay",
    field: "overlayQuickProcessing",
    get keywords() {
      return uiMessage("ui.453173c490");
    },
  },
  {
    get label() {
      return uiMessage("ui.8c90995135");
    },
    section: "privacy",
    field: "retentionDays",
    get keywords() {
      return uiMessage("ui.24e5d30091");
    },
  },
  {
    get label() {
      return uiMessage("ui.b3aaca3b2f");
    },
    section: "privacy",
    field: "trainerEnabled",
    get keywords() {
      return uiMessage("ui.b2d5e4d1ac");
    },
  },
  {
    get label() {
      return uiMessage("ui.18d54d8fce");
    },
    section: "general",
    field: "autostart",
    get keywords() {
      return uiMessage("ui.81209c6c88");
    },
  },
  {
    get label() {
      return uiMessage("ui.fa7b9c286e");
    },
    section: "general",
    field: "insertion",
    get keywords() {
      return uiMessage("ui.f774b2a92f");
    },
  },
  {
    get label() {
      return uiMessage("ui.bb14754e2f");
    },
    section: "general",
    field: "about",
    get keywords() {
      return uiMessage("ui.3869827aa0");
    },
  },
  {
    get label() {
      return uiMessage("ui.406ee0f68c");
    },
    section: "general",
    field: "updates",
    get keywords() {
      return uiMessage("ui.cd0d5ed3ee");
    },
  },
  {
    get label() {
      return uiMessage("ui.e6291756aa");
    },
    section: "diagnostics",
    field: "logs",
    get keywords() {
      return uiMessage("ui.48993dee99");
    },
  },
];

export const relocatedSettings: Record<string, Section> = {
  recordingSoundsEnabled: "activation",
  recordingSoundsVolume: "activation",
  trainerModel: "privacy",
  trainerProfile: "privacy",
  trainerScope: "privacy",
  trainerAiEnabled: "privacy",
  cloudConsent: "privacy",
  "wake-samples": "diagnostics",
};
export const settingFocusFallbacks: Record<string, string> = {
  toggleAutoStopMs: "hotkeyMode",
  selectionHotkey: "selectionCaptureEnabled",
  historyRetentionDays: "historyEnabled",
  recordingSoundsVolume: "recordingSoundsEnabled",
};
searchIndex.push(
  {
    get label() {
      return uiMessage("settings.models");
    },
    section: "audio",
    field: "models",
    keywords:
      "модели загрузить скачать удалить GigaAM Parakeet Whisper download install models",
  },
  {
    get label() {
      return uiMessage("ui.c2ccfcfe69");
    },
    section: "audio",
    field: "acceleration",
    keywords: "ускорение процессор видеокарта CPU GPU Vulkan CUDA acceleration",
  },
  {
    get label() {
      return uiMessage("settings.decoding");
    },
    section: "audio",
    field: "audio-advanced",
    keywords: "потоки декодирование точность CPU threads decoding",
  },
  {
    get label() {
      return uiMessage("wakeLive.title");
    },
    section: "activation",
    field: "wake-live",
    keywords: "привет лира lira wakeword пробуждение бета beta",
  },
  {
    get label() {
      return uiMessage("settings.wakeTools");
    },
    section: "diagnostics",
    field: "wake-samples",
    keywords: "лира образцы запись негативы wakeword samples recording",
  },
  {
    get label() {
      return uiMessage("ui.ea7426e01f");
    },
    section: "overlay",
    field: "overlayOpacity",
    keywords: "непрозрачность прозрачность opacity transparency",
  },
  {
    get label() {
      return uiMessage("settings.overlayTest");
    },
    section: "overlay",
    field: "overlay-preview",
    keywords: "пример состояния индикатор overlay preview test",
  },
  {
    get label() {
      return uiMessage("ui.ee002eb6ca");
    },
    section: "diagnostics",
    field: "verboseLogging",
    keywords: "подробный журнал логи verbose logs logging",
  },
  {
    get label() {
      return uiMessage("ui.930f9a65fb");
    },
    section: "diagnostics",
    field: "feedback",
    keywords: "обратная связь сообщить проблема feedback report",
  },
);

searchIndex.push({
  get label() {
    return uiMessage("applicationContext.setting");
  },
  section: "privacy",
  field: "applicationContextEnabled",
  keywords:
    "приложения контекст статистика вставка application context usage privacy",
});
searchIndex.push({
  get label() {
    return uiMessage("settings.applicationProfiles");
  },
  section: "general",
  field: "application-profiles",
  get keywords() {
    return uiMessage("ui.c45632e45d");
  },
});
searchIndex.push({
  get label() {
    return uiMessage("settings.snippets");
  },
  section: "audio",
  field: "snippets",
  get keywords() {
    return uiMessage("ui.4aae390f18");
  },
});
searchIndex.push({
  get label() {
    return uiMessage("ui.1c4579942e");
  },
  section: "audio",
  field: "pretranscriptionEnabled",
  get keywords() {
    return uiMessage("ui.0d760da869");
  },
});

if (LIVE_DICTATION_ENABLED)
  searchIndex.unshift({
    get label() {
      return uiMessage("ui.a6d9bf3c4d");
    },
    section: "activation",
    field: "dictationMode",
    get keywords() {
      return uiMessage("ui.ab1b250ccf");
    },
  });

if (!(typeof __FONO_WAKE_WORD__ !== "undefined" && __FONO_WAKE_WORD__)) {
  const excluded = new Set(["wakePhrase", "silenceMs", "advanced"]);
  for (let index = searchIndex.length - 1; index >= 0; index--) {
    if (excluded.has(searchIndex[index].field)) searchIndex.splice(index, 1);
  }
}
