<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import PreferenceToggle from "./PreferenceToggle.vue";
import StatusDot from "../../../shared/presentation/StatusDot.vue";
import { FeedbackCard } from "../../feedback";
import { DiagnosticReportCard } from "../../diagnostic-report";
import SettingsDisclosure from "../../../shared/presentation/SettingsDisclosure.vue";
import WakeSamplesPanel from "../../wake-samples/presentation/WakeSamplesPanel.vue";
const workspace = useWorkspace();
const { run, busy } = useFeedback();
const checks = computed(() => workspace.diagnostics());
const build = __FONO_FRONTEND_BUILD__;
async function checkAll() {
  if (workspace.refresh) {
    await workspace.refresh();
    return;
  }
  await new Promise<void>((resolve) => setTimeout(resolve, 500));
  workspace.state.logs.unshift(uiMessage("ui.2c794ce8a4"));
}
</script>
<template>
  <div class="form-stack">
    <div class="section-header">
      <h3>{{ $t("ui.1e8466ebeb") }}</h3>
      <WlButton
        size="sm"
        :loading="busy"
        @click="run(checkAll, $t('ui.880c43c534'))"
        >{{ $t("ui.5ef9db5fd5") }}</WlButton
      >
    </div>
    <div v-for="check in checks" :key="check.id" class="diagnostic-row">
      <div>
        <strong>{{ check.title }}</strong>
        <p>{{ check.detail }}</p>
      </div>
      <StatusDot :health="check.health" /><RouterLink
        v-if="check.health !== 'ready'"
        class="text-link"
        :to="
          check.id === 'service' ? '/api/tasks' : '/settings/' + check.section
        "
        >{{ $t("ui.d71875872c") }}</RouterLink
      >
    </div>
    <DiagnosticReportCard :port="workspace.report" :copy="workspace.copy" />
    <SettingsDisclosure id="diagnostics-advanced">
      <PreferenceToggle
        id="verboseLogging"
        name="verboseLogging"
        :label="$t('ui.ee002eb6ca')"
        :description="$t('ui.37086518bd')"
      />
      <SettingsDisclosure id="logs" :title="$t('ui.97c459a6bb')">
        <div class="section-header">
          <h3>{{ $t("ui.97c459a6bb") }}</h3>
          <WlButton
            size="sm"
            @click="
              run(
                () => workspace.copy(workspace.state.logs.join('\n')),
                $t('ui.f477e92d6e'),
              )
            "
            >{{ $t("ui.1ecddf40bf") }}</WlButton
          >
        </div>
        <pre class="log-view">{{
          workspace.state.logs.slice(0, 20).join("\n")
        }}</pre>
      </SettingsDisclosure>
      <SettingsDisclosure id="build-info" :title="$t('settings.technical')">
        <div class="technical-info">
          <span>Fono {{ build.version }}</span
          ><span>Vue 3 · WhiteUI 0.6.0</span><code>{{ build.revision }}</code
          ><span>{{
            workspace.native ? "Tauri · Rust" : "Browser mock runtime"
          }}</span>
        </div>
      </SettingsDisclosure>
      <SettingsDisclosure id="feedback" :title="$t('ui.930f9a65fb')"
        ><FeedbackCard
      /></SettingsDisclosure>
      <SettingsDisclosure
        v-if="workspace.native && workspace.wakeSamples.state.live.available"
        id="wake-tools"
        :title="$t('settings.wakeTools')"
      >
        <WakeSamplesPanel />
      </SettingsDisclosure>
    </SettingsDisclosure>
  </div>
</template>
