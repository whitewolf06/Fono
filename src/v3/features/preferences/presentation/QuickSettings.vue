<script setup lang="ts">
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
const props = defineProps<{ panel: QuickPanel }>();
const ui = useInteraction();
const router = useRouter();
const { run, busy, error } = useFeedback();
const { draft, dirty, save, canLeave } = useDraft(() => quickKeys[props.panel]);
const closing = ref(false);
const titles: Record<QuickPanel, string> = {
  microphone: "Микрофон",
  wake: "Пробуждение",
  recognition: "Распознавание",
  processing: "Обработка текста",
  hotkey: "Горячая клавиша",
  "command-hotkey": "Голосовые команды · клавиша",
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
        panel === "recognition" ? "Управление моделями" : "Все настройки"
      }}</WlButton
      ><WlButton :disabled="busy" @click="close">Отмена</WlButton
      ><WlButton
        variant="primary"
        :loading="busy"
        :disabled="!dirty"
        @click="
          run(save, 'Настройки сохранены').then((ok) => {
            if (ok && !dirty) ui.state.quick = null;
          })
        "
        >Сохранить</WlButton
      ></template
    >
  </WlDialog>
</template>
