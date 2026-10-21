<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

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
          <AppIcon name="lightning" :size="13" />{{ $t("ui.217a3eef75") }}
        </div>
        <h1 class="hero-title" aria-label="Fono"><FonoWordmark /></h1>
        <p>{{ $t("ui.b079cfbcdc") }}</p>
        <WlButton
          size="sm"
          variant="soft"
          :aria-label="$t('ui.e3032720b1') + triggerLabel(state.preferences)"
          @click="ui.openQuick('hotkey')"
          >{{ triggerLabel(state.preferences)
          }}<template #icon
            ><AppIcon name="edit" :size="12" /></template></WlButton
        ><small class="hero-hint">{{
          state.preferences.hotkeyMode === "toggle"
            ? ["right_alt", "right_ctrl", "caps_lock"].includes(
                state.preferences.dictationTrigger,
              )
              ? $t("ui.d9dd621d98")
              : $t("ui.89a4e4659f")
            : $t("ui.5ffda55a3e")
        }}</small>
      </div>
    </header>
    <VoiceTools /><HomeServices /><RecordControl /><HomeTextPreview />
    <footer v-if="!workspace.native" class="home-footnote">
      {{ $t("ui.69371c1b2a") }}
    </footer>
  </div>
</template>
