<script setup lang="ts">
import { WlIcon, type WlIconName } from "@whitelife-core/ui-kit";
defineProps<{
  name:
    | WlIconName
    | "copy"
    | "server"
    | "stop"
    | "undo"
    | "download"
    | "keyboard"
    | "external-link"
    | "translate";
  size?: number;
}>();
const paths: Record<string, string> = {
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
