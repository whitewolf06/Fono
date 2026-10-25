<script setup lang="ts">
import { WlIcon, type WlIconName } from "@whitelife-core/ui-kit";
import type { AppIconName } from "../domain/icons";
defineProps<{
  name: AppIconName;
  size?: number;
}>();
const paths: Record<string, string> = {
  "audio-file": "M14 3H5v18h14V8z M14 3v5h5 M8 13v3 M11 11v7 M14 13v3",
  history: "M3 11a9 9 0 119 10 M3 4v7h7 M12 7v5l3 2",
  chart: "M4 3v17h17 M8 16v-5 M13 16V7 M18 16V4",
  "voice-command": "M4 4h16v12H9l-5 4z M8 9v2 M11 7v6 M14 9v2 M17 8v4",
  shield: "M12 3l8 3v6c0 4-4 7-8 9-4-2-8-5-8-9V6z M9 12l2 2 4-4",
  sliders:
    "M4 6h7 M15 6h5 M4 12h3 M11 12h9 M4 18h11 M19 18h1 M11 3v6 M7 9v6 M15 15v6",
  terminal: "M3 4h18v16H3z M7 8l4 4-4 4 M14 16h3",
  globe:
    "M21 12a9 9 0 11-18 0 9 9 0 0118 0 M3 12h18 M12 3c-5 5-5 13 0 18 M12 3c5 5 5 13 0 18",
  "external-link": "M14 3h7v7 M21 3L10 14 M10 3H3v18h18v-7",
  copy: "M8 8h12v13H8z M16 8V3H3v13h5",
  server:
    "M4 3h16v7H4z M4 14h16v7H4z M7 6.5h.01 M7 17.5h.01 M11 6.5h6 M11 17.5h6",
  stop: "M6 6h12v12H6z",
  undo: "M8 4L3 9l5 5 M3 9h10a7 7 0 010 14",
  download: "M12 3v12 M7 10l5 5 5-5 M4 17v4h16v-4",
  keyboard:
    "M3 6h18v12H3z M6 9h.01 M10 9h.01 M14 9h.01 M18 9h.01 M6 12h.01 M10 12h.01 M14 12h.01 M18 12h.01 M8 15h8",
  translate:
    "M3 5h11 M8 3v2 M11 5c-1 5-3.5 8-8 10 M5 8c1 3 3.5 5.5 7 7 M13 21l4-10 4 10 M14.5 17h5",
};
</script>
<template>
  <svg
    v-if="paths[name]"
    :width="size || 18"
    :height="size || 18"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.5"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <path :d="paths[name]" />
  </svg>
  <WlIcon v-else :name="name as WlIconName" :size="size || 18" />
</template>
