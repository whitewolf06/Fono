<script setup lang="ts">
import {
  message as uiMessage,
  message as $t,
} from "../../../shared/domain/messages";

import { computed, ref, onScopeDispose } from "vue";
import { useRouter } from "vue-router";
import { WlDialog, WlButton } from "@whitelife-core/ui-kit";
import type { QuickPanel } from "../../../shared/domain/contracts";
import { quickKeys, quickSections } from "../domain/preferences";
import { useDraft } from "../application/useDraft";
import { useInteraction } from "../../../shared/application/interaction";
import { useFeedback } from "../../../shared/application/feedback";
import AudioFields from "./AudioFields.vue";
import ActivationFields from "./ActivationFields.vue";
import ProcessingFields from "./ProcessingFields.vue";
import { WAKE_WORD_AVAILABLE } from "../../../shared/domain/wakeAvailability";
import { useWorkspace } from "../../../shared/application/workspace";
const workspace = useWorkspace();
const props = defineProps<{ panel: QuickPanel }>();
const ui = useInteraction();
const router = useRouter();
const { run, busy, error } = useFeedback();
const { draft, dirty, save, canLeave } = useDraft(() => quickKeys[props.panel]);
const closing = ref(false);
const titles: Record<QuickPanel, string> = {
  get microphone() {
    return uiMessage("ui.f038415295");
  },
  get wake() {
    return uiMessage("ui.8cf77f0854");
  },
  get recognition() {
    return uiMessage("ui.7894eb1ac6");
  },
  get processing() {
    return uiMessage("ui.d4a26d3502");
  },
  get hotkey() {
    return uiMessage("ui.18fb8ccf7e");
  },
  get "command-hotkey"() {
    return uiMessage("ui.d24867ef39");
  },
};
async function close() {
  if (closing.value || busy.value) return;
  closing.value = true;
  if (await canLeave()) ui.state.quick = null;
  closing.value = false;
}
const visible = computed({
  get: () => true,
  set: () => {
    void close();
  },
});
async function allSettings() {
  if (!(await canLeave())) return;
  ui.state.quick = null;
  removeGuard();
  await router.push("/settings/" + quickSections[props.panel]);
}
const removeGuard = router.beforeEach(async () => {
  if (busy.value) return false;
  if (!(await canLeave())) return false;
  ui.state.quick = null;
});
onScopeDispose(removeGuard);
</script>
<template>
  <WlDialog v-model:visible="visible" :header="titles[panel]" width="520px">
    <AudioFields
      v-if="panel === 'microphone' || panel === 'recognition'"
      v-model="draft"
      :scope="panel"
    />
    <ActivationFields
      v-else-if="
        panel === 'wake' || panel === 'hotkey' || panel === 'command-hotkey'
      "
      v-model="draft"
      :scope="panel"
    />
    <ProcessingFields v-else v-model="draft" />
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
    <template #footer
      ><WlButton variant="link" class="push-left" @click="allSettings">{{
        panel === "recognition" ? $t("ui.3fe343fdd3") : $t("ui.f9a245781f")
      }}</WlButton
      ><WlButton :disabled="busy" @click="close">{{
        $t("ui.8fbe9b75cb")
      }}</WlButton
      ><WlButton
        variant="primary"
        :loading="busy"
        :disabled="
          !dirty ||
          workspace.state.triggerCaptureActive ||
          (panel === 'wake' && !WAKE_WORD_AVAILABLE)
        "
        @click="
          run(save, $t('ui.babdb58139')).then((ok) => {
            if (ok && !dirty) ui.state.quick = null;
          })
        "
        >{{ $t("ui.b4d30cae52") }}</WlButton
      ></template
    >
  </WlDialog>
</template>
