<script setup lang="ts">
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
const workspace = useWorkspace();
const { run } = useFeedback();
import { WlSlider, WlButton } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import PreferenceToggle from "./PreferenceToggle.vue";
import { OverlayPreview } from "../../overlay";
import SelectField from "../../../shared/presentation/SelectField.vue";
const draft = defineModel<Preferences>({ required: true });
</script>
<template>
  <div class="form-stack">
    <PreferenceToggle
      name="overlayEnabled"
      label="Показывать индикатор записи"
      description="Состояние диктовки поверх других приложений."
    /><PreferenceToggle
      name="overlayCompact"
      label="Компактный вид"
      description="Только статус и основные действия."
    /><label id="overlayScale"
      >Масштаб · {{ draft.overlayScale }}%<WlSlider
        v-model="draft.overlayScale"
        :min="70"
        :max="130"
        :step="5"
        aria-label="Масштаб индикатора" /></label
    ><label
      >Непрозрачность · {{ draft.overlayOpacity }}%<WlSlider
        v-model="draft.overlayOpacity"
        :min="30"
        :max="100"
        :step="5"
        aria-label="Непрозрачность индикатора"
    /></label>
    <SelectField
      v-model="draft.overlayPosition"
      label="Положение"
      :options="[
        { value: 'custom', label: 'Текущее положение' },
        { value: 'bottom', label: 'Внизу по центру' },
        { value: 'top', label: 'Вверху по центру' },
      ]"
    />
    <div class="preview-stage" :data-position="draft.overlayPosition">
      <OverlayPreview :preferences="draft" /><small
        >Предпросмотр изменяется сразу. Для применения нажмите
        «Сохранить».</small
      >
    </div>
    <div class="actions">
      <WlButton
        v-if="workspace.settings.previewOverlay"
        size="sm"
        @click="run(() => workspace.settings.previewOverlay!(draft))"
        >Показать поверх окон</WlButton
      >
      <WlButton
        size="sm"
        @click="
          workspace.settings.resetOverlay
            ? run(() => workspace.settings.resetOverlay!())
            : (draft.overlayPosition = 'bottom')
        "
        >Сбросить положение</WlButton
      ><RouterLink class="text-link" to="/overlay"
        >Все состояния индикатора</RouterLink
      >
    </div>
  </div>
</template>
