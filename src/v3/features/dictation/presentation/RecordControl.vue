<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed, defineAsyncComponent } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import {
  useWorkspace,
  phaseLabels,
} from "../../../shared/application/workspace";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import VoiceWave from "./VoiceWave.vue";
import { liveStatus } from "../domain/live";
import { useFeedback } from "../../../shared/application/feedback";
import PendingDictationActions from "../../../shared/presentation/PendingDictationActions.vue";
import { copyPendingText } from "../application/copyPendingText";
import DictationNotice from "../../../shared/presentation/DictationNotice.vue";
import { dictationNotice } from "../../../shared/domain/dictationNotice";
import { message } from "../../../shared/domain/messages";
const liveCompiled =
  typeof __FONO_EXPERIMENTAL_LIVE__ !== "undefined" &&
  __FONO_EXPERIMENTAL_LIVE__;
const LiveRecordingStatus =
  typeof __FONO_EXPERIMENTAL_LIVE__ !== "undefined" &&
  __FONO_EXPERIMENTAL_LIVE__
    ? defineAsyncComponent(() => import("./LiveRecordingStatus.vue"))
    : null;
const { run } = useFeedback();
const workspace = useWorkspace();
const pending = computed(() => {
  const value = workspace.state.pendingDictation;
  return value
    ? { ...value, error: workspace.state.error || value.error }
    : null;
});
const notice = computed(() => dictationNotice(workspace.state.error));
const live = computed(() => workspace.state.live);
const liveMode = computed(
  () => workspace.state.preferences.dictationMode === "live",
);
const status = computed(() =>
  workspace.state.dictationOutcome === "no_speech"
    ? message("dictation.noSpeech")
    : liveMode.value && live.value
      ? liveStatus(live.value)
      : phaseLabels[workspace.state.phase],
);
const recording = computed(() =>
  ["listening", "silence"].includes(workspace.state.phase),
);
const working = computed(() =>
  ["transcribing", "processing"].includes(workspace.state.phase),
);
async function copyPending(text: string, sessionId: number) {
  await run(
    () =>
      copyPendingText(
        {
          pending: () => workspace.state.pendingDictation,
          copy: workspace.copy,
          resolve: workspace.dictation.resolvePending,
        },
        text,
        sessionId,
      ),
    uiMessage("ui.d2d08c715e"),
  );
}
</script>
<template>
  <section class="record-area" :aria-label="$t('ui.7a38a67834')">
    <VoiceWave
      v-if="!pending && !notice"
      :phase="workspace.state.phase"
      :level="workspace.state.audioLevel"
    />
    <PendingDictationActions
      v-if="pending"
      :pending="pending"
      @resolve="
        (request) => run(() => workspace.dictation.resolvePending(request))
      "
      @copy="copyPending"
    />
    <DictationNotice v-else-if="notice" :notice="notice" />
    <div v-if="!pending" class="record-controls">
      <span class="record-status" role="status" v-if="!notice"
        >{{ status
        }}<span v-if="recording" class="mono"
          >{{
            Math.floor(workspace.state.elapsed / 60)
              .toString()
              .padStart(2, "0")
          }}:{{
            Math.floor(workspace.state.elapsed % 60)
              .toString()
              .padStart(2, "0")
          }}</span
        ></span
      ><WlButton
        size="sm"
        :variant="recording ? 'soft-danger' : 'soft'"
        :loading="working"
        @click="
          run(() =>
            recording
              ? workspace.dictation.finish()
              : workspace.dictation.start(),
          )
        "
        ><template #icon
          ><AppIcon
            :name="recording ? 'stop' : 'microphone'"
            :size="16" /></template
        >{{
          recording
            ? $t("ui.8be2bbf39a")
            : working
              ? $t("ui.fd0ecd47a7")
              : $t("ui.10b7c794e6")
        }}</WlButton
      ><WlButton
        v-if="recording || working"
        size="sm"
        variant="ghost"
        @click="run(() => workspace.dictation.cancel())"
        >{{ $t("ui.8fbe9b75cb") }}</WlButton
      >
    </div>
    <LiveRecordingStatus v-if="liveCompiled" />
  </section>
</template>
