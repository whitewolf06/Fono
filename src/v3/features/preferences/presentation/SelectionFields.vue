<script setup lang="ts">
import { WlButton, WlField, WlInput } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import { useWorkspace } from "../../../shared/application/workspace";
import PreferenceToggle from "./PreferenceToggle.vue";
const workspace = useWorkspace();
defineProps<{ active: boolean }>();
const draft = defineModel<Preferences>({ required: true });
</script>
<template>
  <section class="form-stack">
    <h3>Получение выделенного текста</h3>
    <PreferenceToggle
      name="selectionCaptureEnabled"
      :disabled="active || workspace.state.triggerCaptureActive"
      label="Отдельное сочетание для выделения"
      description="Получить только выбранный фрагмент другого приложения. По умолчанию выключено."
    />
    <WlField
      id="selectionHotkey"
      v-slot="field"
      label="Сочетание для выделения"
      hint="Выделите фрагмент в другом приложении, нажмите и отпустите сочетание. Fono покажет полученный текст. Повторное сочетание закрывает его."
    >
      <WlInput
        v-bind="field"
        v-model="draft.selectionHotkey"
        :disabled="active || workspace.state.triggerCaptureActive"
      />
    </WlField>
    <PreferenceToggle
      name="selectionClipboardFallback"
      :disabled="active || workspace.state.triggerCaptureActive"
      label="Разрешить резервный Ctrl+C"
      description="Если UI Automation недоступен, Fono может скопировать подтверждённое выделение и восстановить буфер. Стороннее приложение может добавить копию в историю буфера Windows."
    />
    <p class="muted">
      Без подтверждённого выделения и в защищённых полях получение отклоняется.
      Сочетание не должно пересекаться с диктовкой и голосовыми командами.
    </p>
    <WlButton
      v-if="!workspace.native && workspace.selection.demo"
      @click="workspace.selection.demo()"
      >Посмотреть пример выделения</WlButton
    >
    <p v-if="!workspace.native" class="notice">
      В браузере доступен только пример формы. Другие приложения и буфер обмена
      не читаются.
    </p>
  </section>
</template>
