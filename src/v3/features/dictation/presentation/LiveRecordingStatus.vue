<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

import { computed } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { triggerLabel } from "../../../shared/domain/triggers";
import { useFeedback } from "../../../shared/application/feedback";
const workspace = useWorkspace();
const { run } = useFeedback();
const live = computed(() => workspace.state.live);
const liveMode = computed(
  () => workspace.state.preferences.dictationMode === "live",
);
const recording = computed(() =>
  ["listening", "silence"].includes(workspace.state.phase),
);
const working = computed(() =>
  ["transcribing", "processing"].includes(workspace.state.phase),
);
const canResume = computed(
  () =>
    live.value?.phase === "listening" &&
    ["paused_focus", "none"].includes(live.value.insertionState),
);
</script>
<template>
  <p v-if="liveMode && !recording && !working" class="live-notice">
    {{ $t("ui.80d6ce02bf") }} {{ triggerLabel(workspace.state.preferences)
    }}{{ $t("ui.f8cbcc9fbf") }}
  </p>
  <p
    v-if="live && recording"
    class="live-notice"
    :class="{
      warning: live.insertionState === 'failed' || live.lagMs > 4000,
    }"
    role="status"
  >
    {{
      live.warning ||
      (live.insertionState === "paused_focus"
        ? $t("ui.a8527eee62")
        : live.insertionState === "none"
          ? $t("ui.e854db5c2c")
          : live.insertionState === "failed"
            ? $t("ui.1069f0fb9c")
            : live.lagMs > 4000
              ? $t("ui.5be18284fe")
              : $t("ui.68a473fb77"))
    }}
  </p>
  <p v-if="canResume && !workspace.native" class="live-notice">
    <WlButton
      size="xs"
      variant="ghost"
      @click="run(() => workspace.dictation.resumeInsertion())"
      >{{ $t("ui.8abddba0c1") }}</WlButton
    >
  </p>
</template>
