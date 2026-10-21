<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

import AppIcon from "../../../shared/presentation/AppIcon.vue";
withDefaults(
  defineProps<{ hotkeyMode?: "hold" | "toggle"; dictationTrigger?: string }>(),
  {
    hotkeyMode: "toggle",
  },
);
</script>
<template>
  <p>
    <AppIcon name="check" :size="15" class="accept" /><span
      ><strong>{{ $t("ui.543f6aa1e0") }}</strong>
      {{ $t("ui.9aec937ec1") }}</span
    >
  </p>
  <p>
    <AppIcon name="copy" :size="15" class="copy" /><span
      ><strong>{{ $t("ui.1ecddf40bf") }}</strong>
      {{ $t("ui.b295a5713c") }}</span
    >
  </p>
  <p>
    <AppIcon name="x" :size="15" class="cancel" /><span
      ><strong>{{ $t("ui.7110a16ab9") }}</strong>
      {{ $t("ui.029ad49f6d") }}</span
    >
  </p>
  <p>
    <AppIcon name="keyboard" :size="15" /><span
      ><strong>{{
        hotkeyMode === "hold"
          ? $t("ui.c95c4b3501")
          : ["right_alt", "right_ctrl", "caps_lock"].includes(
                dictationTrigger || "",
              )
            ? $t("ui.eb6048f843")
            : $t("ui.2fe255b1d1")
      }}</strong>
      {{ $t("ui.2932e78e80") }}</span
    >
  </p>
  <p>
    <AppIcon name="sparkle" :size="15" class="processing" /><span
      ><strong>{{ $t("ui.416f521a2f") }}</strong>
      {{ $t("ui.0251b2225e") }}</span
    >
  </p>
  <p>
    <AppIcon name="translate" :size="15" class="translation" /><span
      ><strong>{{ $t("ui.c581c83538") }}</strong>
      {{ $t("ui.5b0d0f6e03") }}</span
    >
  </p>
  <small>{{ $t("ui.0098bf6082") }}</small>
</template>
