<script setup lang="ts">
import { WlField, WlSelect } from "@whitelife-core/ui-kit";
defineProps<{
  label: string;
  options: unknown[];
  hint?: string;
  id?: string;
  disabled?: boolean;
}>();
const model = defineModel<string | number>({ required: true });
function update(value: unknown) {
  if (typeof value === "string" || typeof value === "number")
    model.value = value;
}
</script>
<template>
  <WlField v-slot="field" :label="label" :hint="hint" :id="id"
    ><WlSelect
      v-bind="field"
      :aria-label="label"
      :model-value="model"
      :options="options"
      option-label="label"
      option-value="value"
      :disabled="disabled"
      @update:model-value="update"
  /></WlField>
</template>
