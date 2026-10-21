import { uiIntlLocale } from "../../../shared/domain/locale";
import { message as uiMessage } from "../../../shared/domain/messages";
import type { Dictation } from "../../../shared/domain/contracts";

export function exactDateLabel(date: string): string {
  return new Date(date).toLocaleString(uiIntlLocale(), {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function durationLabel(milliseconds?: number | null): string {
  if (
    milliseconds == null ||
    !Number.isFinite(milliseconds) ||
    milliseconds < 0
  )
    return uiMessage("ui.c5805f8b9d");
  if (milliseconds < 1000)
    return Math.round(milliseconds) + uiMessage("ui.3d28ff50f5");
  return (
    new Intl.NumberFormat(uiIntlLocale(), { maximumFractionDigits: 2 }).format(
      milliseconds / 1000,
    ) + uiMessage("ui.ddae8eb8bf")
  );
}

export function recordingLabel(entry: Dictation): string {
  const measured = entry.metadata?.recordingDurationMs;
  if (measured != null) return durationLabel(measured);
  return entry.duration > 0
    ? durationLabel(entry.duration * 1000)
    : uiMessage("ui.434f2cd9ac");
}

export function dictationFacts(
  entry: Dictation,
): { title: string; value: string }[] {
  const m = entry.metadata;
  const facts = [
    {
      get title() {
        return uiMessage("ui.1d80e1757a");
      },
      value: recordingLabel(entry),
    },
    {
      get title() {
        return uiMessage("ui.b7f32afbe8");
      },
      value: durationLabel(m?.generationDurationMs),
    },
    {
      get title() {
        return uiMessage("ui.7894eb1ac6");
      },
      value: durationLabel(m?.recognitionDurationMs),
    },
    {
      get title() {
        return uiMessage("ui.d4df82f58c");
      },
      value: m?.backend
        ? { cpu: "CPU", cuda: "NVIDIA CUDA", vulkan: "Vulkan" }[m.backend]
        : uiMessage("ui.b5eede8dfc"),
    },
    {
      get title() {
        return uiMessage("ui.f3bdf7cce3");
      },
      value: m?.model || uiMessage("ui.bf2a878be5"),
    },
  ];
  if (m?.requestedAcceleration)
    facts.push({
      get title() {
        return uiMessage("ui.8e4f4c60d3");
      },
      value: {
        get auto() {
          return uiMessage("ui.8dca2f7831");
        },
        cpu: "CPU",
        cuda: "NVIDIA CUDA",
        vulkan: "Vulkan",
      }[m.requestedAcceleration],
    });
  if (m?.language)
    facts.push({
      get title() {
        return uiMessage("ui.5bd57038db");
      },
      value:
        {
          get auto() {
            return uiMessage("ui.8dca2f7831");
          },
          get ru() {
            return uiMessage("ui.6da140fdd4");
          },
          get en() {
            return uiMessage("ui.e7c1ef1d8e");
          },
        }[m.language] || m.language,
    });
  if (m?.processingMode)
    facts.push({
      get title() {
        return uiMessage("ui.5dc9695bc0");
      },
      value: {
        get off() {
          return uiMessage("ui.1e4b686990");
        },
        get raw() {
          return uiMessage("ui.4274a06c5d");
        },
        get clean() {
          return uiMessage("ui.b0f05e6f3d");
        },
        get format() {
          return uiMessage("ui.65f12809f6");
        },
        get command() {
          return uiMessage("ui.8b372c12c2");
        },
        get task() {
          return uiMessage("ui.77102f0c76");
        },
        get formal() {
          return uiMessage("ui.f7c565c215");
        },
      }[m.processingMode],
    });
  if (m?.processingTrigger)
    facts.push({
      get title() {
        return uiMessage("ui.c0fe5b7e9a");
      },
      value:
        m.processingTrigger === "manual"
          ? uiMessage("ui.66a8160b08")
          : uiMessage("ui.353cf589e9"),
    });
  if (m?.processingTranslation)
    facts.push({
      get title() {
        return uiMessage("ui.ea6f9eb94d");
      },
      value: {
        get en() {
          return uiMessage("ui.e7c1ef1d8e");
        },
        get ru() {
          return uiMessage("ui.6da140fdd4");
        },
        get de() {
          return uiMessage("ui.79c7568c22");
        },
        get fr() {
          return uiMessage("ui.7518ed2eef");
        },
        get es() {
          return uiMessage("ui.5097d628b8");
        },
      }[m.processingTranslation],
    });
  if (m?.dictionaryEnabled !== undefined)
    facts.push({
      get title() {
        return uiMessage("ui.4e7867b548");
      },
      value: m.dictionaryEnabled
        ? uiMessage("ui.0b81629905")
        : uiMessage("ui.5a577d9056"),
    });
  return facts;
}

export function dictationStages(
  entry: Dictation,
): { title: string; value: string }[] {
  const m = entry.metadata;
  const stages = [];
  for (const [title, duration] of [
    [uiMessage("ui.7c8a4ae5c5"), m?.modelLoadDurationMs],
    [uiMessage("ui.d4a26d3502"), m?.processingDurationMs],
  ] as const) {
    if (duration != null && Number.isFinite(duration) && duration >= 0)
      stages.push({ title, value: durationLabel(duration) });
  }
  if (m?.detectedLanguage)
    stages.push({
      get title() {
        return uiMessage("ui.7ad22789d3");
      },
      value:
        {
          get ru() {
            return uiMessage("ui.6da140fdd4");
          },
          get en() {
            return uiMessage("ui.e7c1ef1d8e");
          },
        }[m.detectedLanguage] || m.detectedLanguage,
    });
  return stages;
}
