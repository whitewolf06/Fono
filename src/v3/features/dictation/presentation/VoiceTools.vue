<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed } from "vue";
import { WlButton, WlSwitch, type WlIconName } from "@whitelife-core/ui-kit";
import { useRouter } from "vue-router";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
import { microphones, PreferenceToggle } from "../../preferences";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import StatusDot from "../../../shared/presentation/StatusDot.vue";
import { liraActive } from "../../wake-samples/domain/samples";
import { captureSettingsLocked } from "../../../shared/domain/pretranscription";
import {
  WAKE_WORD_AVAILABLE,
  unavailableWakeMessage,
} from "../../../shared/domain/wakeAvailability";
import type {
  Health,
  QuickPanel,
  ToggleKey,
} from "../../../shared/domain/contracts";
const workspace = useWorkspace();
const ui = useInteraction();
const router = useRouter();
const live = computed(() => workspace.wakeSamples.state.live);
const liveActive = computed(() => liraActive(live.value));
const liveBlocked = computed(
  () =>
    workspace.wakeSamples.state.livePending ||
    live.value.phase === "stopping" ||
    (!liveActive.value &&
      (workspace.wakeSamples.state.phase !== "idle" ||
        workspace.wakeSamples.state.testingId !== null ||
        captureSettingsLocked(workspace.state.phase) ||
        workspace.state.triggerCaptureActive)),
);
function changeLive(value: boolean) {
  if (liveBlocked.value) return;
  void (value
    ? workspace.wakeSamples.enableLive()
    : workspace.wakeSamples.disableLive());
}
function syncLiveToggle(event: Event) {
  (event.currentTarget as HTMLInputElement).checked = liveActive.value;
}
function configure(panel: QuickPanel, liveWake?: boolean) {
  if (liveWake)
    void router.push({
      path: "/settings/activation",
      query: { field: "wake-samples" },
    });
  else ui.openQuick(panel);
}
const tools = computed<
  {
    title: string;
    icon: WlIconName;
    panel: QuickPanel;
    value: string;
    health: Health;
    toggle?: ToggleKey;
    liveWake?: boolean;
  }[]
>(() => {
  const p = workspace.state.preferences;
  const model = workspace.state.models.find((m) => m.id === p.model);
  const translating =
    p.processingTranslationEnabled && p.processingTranslation !== "none";
  const passthrough = p.processingMode === "raw" && !translating;
  return [
    {
      get title() {
        return uiMessage("ui.f038415295");
      },
      icon: "microphone",
      panel: "microphone",
      value: workspace.state.microphoneAvailable
        ? (workspace.state.devices || microphones).find(
            (m) => m.value === p.microphone,
          )?.label || p.microphone
        : uiMessage("ui.08024ace05"),
      health: workspace.state.microphoneAvailable ? "ready" : "missing",
    },
    ...(live.value.available
      ? [
          {
            title: uiMessage("wakeLive.title"),
            icon: "lightning" as const,
            panel: "wake" as const,
            value: uiMessage(`wakeLive.${live.value.phase}`),
            health: (
              {
                off: "off",
                loading: "loading",
                listening: "ready",
                paused: "loading",
                stopping: "loading",
                error: "error",
              } as const
            )[live.value.phase],
            liveWake: true,
          },
        ]
      : typeof __FONO_WAKE_WORD__ !== "undefined" && __FONO_WAKE_WORD__
        ? [
            {
              get title() {
                return uiMessage("ui.8cf77f0854");
              },
              icon: "lightning" as const,
              panel: "wake" as const,
              value: p.wakePhrase,
              health:
                !WAKE_WORD_AVAILABLE || !p.wakeEnabled
                  ? ("off" as Health)
                  : ["loading", "initializing"].includes(
                        workspace.state.wakeStatus || "",
                      )
                    ? "loading"
                    : workspace.state.wakeStatus === "missing_model" ||
                        workspace.state.wakeSetup?.verified === false
                      ? "missing"
                      : workspace.state.wakeStatus?.startsWith("error")
                        ? "error"
                        : "ready",
              toggle: "wakeEnabled" as const,
            },
          ]
        : []),
    {
      get title() {
        return uiMessage("ui.7894eb1ac6");
      },
      icon: "message",
      panel: "recognition",
      value:
        model?.status === "installed"
          ? model.name
          : model?.status === "downloading"
            ? uiMessage("ui.a04b52b1ad") + model.progress + "%"
            : uiMessage("ui.bca25035b9"),
      health:
        model?.status === "installed"
          ? "ready"
          : model?.status === "downloading"
            ? "loading"
            : "missing",
    },
    {
      get title() {
        return uiMessage("ui.d4a26d3502");
      },
      icon: "sparkle",
      panel: "processing",
      value:
        p.dictationMode === "live"
          ? uiMessage("ui.f81164f622")
          : p.processingMode === "raw"
            ? translating
              ? uiMessage("ui.248f7ed17a", {
                  v0: String(p.processingTranslation.toUpperCase()),
                })
              : uiMessage("ui.dc7a475bf1")
            : p.processingModel || uiMessage("ui.c4dac53b9a"),
      health:
        p.dictationMode === "live" || !p.processingEnabled
          ? "off"
          : passthrough
            ? "ready"
            : workspace.native && !workspace.state.aiChecked
              ? p.profile && p.processingModel
                ? "ready"
                : "missing"
              : workspace.state.aiAvailable
                ? "ready"
                : "error",
      toggle: p.dictationMode === "live" ? undefined : "processingEnabled",
    },
  ];
});
</script>
<template>
  <section class="voice-tools" :aria-label="$t('ui.e4fbce7e3c')">
    <article v-for="tool in tools" :key="tool.panel" class="tool-card">
      <div class="tool-title">
        <div class="tool-heading">
          <AppIcon class="tool-icon" :name="tool.icon" />
          <strong>{{ tool.title }}</strong>
        </div>
        <WlSwitch
          v-if="tool.liveWake"
          :model-value="liveActive"
          :disabled="liveBlocked"
          :aria-label="$t(liveActive ? 'wakeLive.disable' : 'wakeLive.enable')"
          :title="$t('wakeLive.hint')"
          @update:model-value="changeLive"
          @change="syncLiveToggle"
        />
        <PreferenceToggle
          v-else-if="tool.toggle"
          :name="tool.toggle"
          :label="tool.title"
          compact
        />
      </div>
      <div class="tool-value" :title="tool.value">{{ tool.value }}</div>
      <p
        v-if="
          tool.liveWake && (live.message || workspace.wakeSamples.state.error)
        "
        class="error-text"
        role="alert"
      >
        {{ live.message || workspace.wakeSamples.state.error }}
      </p>
      <div class="tool-bottom">
        <StatusDot
          :health="tool.health"
          :label="
            tool.panel === 'wake' && !tool.liveWake && !WAKE_WORD_AVAILABLE
              ? $t('ui.09ebf55cf0')
              : undefined
          "
        /><WlButton
          size="xs"
          variant="ghost"
          :aria-label="$t('ui.e692931a28') + tool.title"
          :disabled="
            tool.panel === 'wake' && !tool.liveWake && !WAKE_WORD_AVAILABLE
          "
          :title="
            tool.panel === 'wake' && !tool.liveWake && !WAKE_WORD_AVAILABLE
              ? unavailableWakeMessage()
              : undefined
          "
          @click="configure(tool.panel, tool.liveWake)"
          ><template #icon><AppIcon name="settings" :size="14" /></template
          >{{
            tool.panel === "wake" && !tool.liveWake && !WAKE_WORD_AVAILABLE
              ? $t("ui.72c299e0d6")
              : $t("ui.d71875872c")
          }}</WlButton
        >
      </div>
    </article>
  </section>
</template>
