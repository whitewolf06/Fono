<script setup lang="ts">
import { WlButton } from "@whitelife-core/ui-kit";
import FonoWordmark from "../../../shared/FonoWordmark.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
import VoiceTools from "./VoiceTools.vue";
import HomeServices from "./HomeServices.vue";
import RecordControl from "./RecordControl.vue";
import HomeTextPreview from "./HomeTextPreview.vue";
const { state } = useWorkspace();
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
        <p>Освободите мысли. Fono запишет.</p>
        <WlButton
          size="sm"
          variant="soft"
          :aria-label="'Изменить горячую клавишу ' + state.preferences.hotkey"
          @click="ui.openQuick('hotkey')"
          >{{ state.preferences.hotkey
          }}<template #icon
            ><AppIcon name="edit" :size="12" /></template></WlButton
        ><small class="hero-hint">начать и завершить диктовку</small>
      </div>
    </header>
    <VoiceTools /><HomeServices /><RecordControl /><HomeTextPreview />
    <footer class="home-footnote">
      Демонстрационные данные · настройки интерфейса сохраняются в этом браузере
    </footer>
  </div>
</template>
