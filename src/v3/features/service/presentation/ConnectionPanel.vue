<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed, ref } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
const workspace = useWorkspace();
const { run } = useFeedback();
const revealed = ref(false);
const token = "fono-demo-token-not-a-real-secret";
const address = computed(
  () => "http://" + (workspace.state.serviceAddress || "127.0.0.1:17832"),
);
const example =
  'curl -X POST http://127.0.0.1:17832/v1/transcriptions\n  -H "Authorization: Bearer YOUR_TOKEN"\n  -F "audio=@recording.wav"';
const endpoints = [
  ["GET", "/v1/health", uiMessage("ui.428d46a100")],
  ["POST", "/v1/transcriptions", uiMessage("ui.57ece22d3a")],
  ["GET", "/v1/transcription-jobs/{id}", uiMessage("ui.1e221f3262")],
  ["POST", "/v1/transcription-jobs/{id}/cancel", uiMessage("ui.606e3a5585")],
  ["GET", "/openapi.json", uiMessage("ui.af64c38e80")],
];
</script>
<template>
  <div class="form-stack">
    <section class="surface-panel">
      <h2>{{ $t("ui.a1ec191fce") }}</h2>
      <p v-if="workspace.native">{{ $t("ui.d05fe441be") }}</p>
      <p v-else>{{ $t("ui.fab12f16ce") }}</p>
      <div class="copy-row">
        <code>{{ address }}</code
        ><WlButton
          size="sm"
          @click="run(() => workspace.copy(address), $t('ui.7e435df863'))"
          >{{ $t("ui.e7738e2cb9") }}</WlButton
        >
      </div>
      <div class="section-divider" />
      <h3>{{ $t("ui.7f6fc04a7c") }}</h3>
      <small v-if="!workspace.native" class="muted">{{
        $t("ui.4f371d9eef")
      }}</small>
      <div class="copy-row">
        <code>{{
          !workspace.native && revealed ? token : "••••••••••••••••••••••••"
        }}</code
        ><WlButton
          v-if="!workspace.native"
          size="sm"
          variant="ghost"
          @click="revealed = !revealed"
          ><template #icon
            ><AppIcon :name="revealed ? 'eye-off' : 'eye'" /></template
          >{{ revealed ? $t("ui.0a2a4b8377") : $t("ui.1e5bec3aab") }}</WlButton
        ><WlButton
          size="sm"
          @click="
            run(
              () =>
                workspace.service.copyToken
                  ? workspace.service.copyToken()
                  : workspace.copy(token),
              $t('ui.1a299abb1e'),
            )
          "
          >{{ $t("ui.1ecddf40bf") }}</WlButton
        >
      </div>
    </section>
    <section class="surface-panel">
      <div class="section-header">
        <h2>{{ $t("ui.8a61bbec41") }}</h2>
        <WlButton
          size="sm"
          variant="ghost"
          @click="
            run(
              () =>
                workspace.copy(
                  example.replace('http://127.0.0.1:17832', address),
                ),
              $t('ui.edb3210a75'),
            )
          "
          ><template #icon><AppIcon name="copy" /></template
          >{{ $t("ui.1ecddf40bf") }}</WlButton
        >
      </div>
      <pre>{{ example.replace("http://127.0.0.1:17832", address) }}</pre>
      <p class="muted">{{ $t("ui.36636581a4") }}</p>
    </section>
    <section class="surface-panel">
      <h2>{{ $t("ui.63bcf6456c") }}</h2>
      <div
        v-for="[method, path, description] in endpoints"
        :key="path"
        class="endpoint-row"
      >
        <span class="method" :class="{ post: method === 'POST' }">{{
          method
        }}</span>
        <div>
          <code>{{ path }}</code>
          <p>{{ description }}</p>
        </div>
      </div>
      <div class="copy-row">
        <span>{{ $t("ui.780cd5278b") }} <code>/docs</code></span
        ><WlButton
          size="sm"
          @click="
            run(() => workspace.copy(address + '/docs'), $t('ui.b4039908dc'))
          "
          >{{ $t("ui.db190f8271") }}</WlButton
        >
      </div>
    </section>
  </div>
</template>
