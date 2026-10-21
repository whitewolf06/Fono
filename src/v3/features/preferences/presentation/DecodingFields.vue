<script setup lang="ts">
import { computed } from "vue";
import type { Preferences } from "../../../shared/domain/contracts";
import { message } from "../../../shared/domain/messages";
import SelectField from "../../../shared/presentation/SelectField.vue";
const draft = defineModel<Preferences>({ required: true });
const modes = [
  {
    value: "fast",
    get label() {
      return message("decoding.fast");
    },
  },
  {
    value: "accurate",
    get label() {
      return message("decoding.accurate");
    },
  },
];
const threadOptions = [
  {
    value: "auto",
    get label() {
      return message("decoding.autoThreads");
    },
  },
  ...Array.from({ length: 8 }, (_, index) => ({
    value: String(index + 1),
    label: String(index + 1),
  })),
];
const threads = computed({
  get: () =>
    draft.value.whisperThreads === null
      ? "auto"
      : String(draft.value.whisperThreads),
  set: (value: string | number) => {
    const count = Number(value);
    draft.value.whisperThreads =
      value === "auto"
        ? null
        : Number.isInteger(count) && count >= 1 && count <= 8
          ? count
          : null;
  },
});
</script>
<template>
  <div class="form-grid">
    <SelectField
      id="whisper-decoding"
      v-model="draft.whisperDecoding"
      :label="message('decoding.mode')"
      :options="modes"
      :hint="message('decoding.hint')"
    />
    <SelectField
      id="whisper-threads"
      v-model="threads"
      :label="message('decoding.threads')"
      :options="threadOptions"
      :hint="message('decoding.threadsHint')"
    />
  </div>
</template>
