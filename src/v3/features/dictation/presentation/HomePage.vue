<script setup lang="ts">
import { WlButton } from "@whitelife-core/ui-kit";
import FonoWordmark from "../../../shared/FonoWordmark.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
import { triggerLabel } from "../../../shared/domain/triggers";
import VoiceTools from "./VoiceTools.vue";
import HomeServices from "./HomeServices.vue";
import RecordControl from "./RecordControl.vue";
import HomeTextPreview from "./HomeTextPreview.vue";
const workspace = useWorkspace();
const { state } = workspace;
const ui = useInteraction();
</script>
<template>
  <div class="home-page">
    <header class="home-hero">
      <div class="hero-copy">
        <div class="eyebrow">
          <AppIcon name="lightning" :size="13" />Голосовой ввод
        </div>
        <h1 class="hero-title" aria-label="Fono"><FonoWordmark /></h1>
        <p>Просто записывайте на лету.</p>
        <WlButton
          size="sm"
          variant="soft"
          :aria-label="
            'Изменить кнопку диктовки ' + triggerLabel(state.preferences)
          "
          @click="ui.openQuick('hotkey')"
          >{{ triggerLabel(state.preferences)
          }}<template #icon
            ><AppIcon name="edit" :size="12" /></template></WlButton
        ><small class="hero-hint">{{
          state.preferences.hotkeyMode === "toggle"
            ? ["right_alt", "right_ctrl", "caps_lock"].includes(
                state.preferences.dictationTrigger,
              )
              ? "дважды нажмите для начала · повторите для завершения"
              : "нажмите для начала · ещё раз для завершения"
            : "удерживайте во время речи"
        }}</small>
      </div>
    </header>
    <VoiceTools /><HomeServices /><RecordControl /><HomeTextPreview />
    <footer v-if="!workspace.native" class="home-footnote">
      Демонстрационные данные · настройки интерфейса сохраняются в этом браузере
    </footer>
  </div>
</template>
