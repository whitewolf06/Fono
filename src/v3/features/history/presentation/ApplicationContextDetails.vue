<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";
import type { Dictation } from "../../../shared/domain/contracts";
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
import { useFeedback } from "../../../shared/application/feedback";
import { contextLabel } from "../application/applicationUsage";
const props = defineProps<{ entry: Dictation }>();
const workspace = useWorkspace(),
  ui = useInteraction();
const { run, busy, error } = useFeedback();
async function remove() {
  const entryId = props.entry.id;
  if (
    await ui.confirm({
      title: $t("applicationContext.clearOne"),
      text: $t("applicationContext.clearExplanation"),
      accept: $t("applicationContext.clear"),
      danger: true,
    })
  )
    await run(() => workspace.history.clearApplicationContext(entryId));
}
</script>
<template>
  <section
    class="application-context"
    :aria-label="$t('applicationContext.title')"
  >
    <p>
      <strong>{{
        entry.applicationContext?.before?.name || $t("applicationContext.title")
      }}</strong>
      · {{ contextLabel(entry) }}
    </p>
    <p v-if="entry.applicationContext?.attempted" class="muted">
      {{ $t("applicationContext.unconfirmed") }}
      <template v-if="entry.applicationContext.method">
        {{
          $t(`applicationContext.${entry.applicationContext.method}`)
        }}</template
      >
    </p>
    <WlButton
      v-if="entry.applicationContext"
      size="sm"
      variant="danger-quiet"
      :disabled="busy"
      @click="remove"
      >{{ $t("applicationContext.clearOne") }}</WlButton
    >
    <p v-if="error" class="error-text" role="alert">{{ error }}</p>
  </section>
</template>
<style scoped>
.application-context {
  border-block-end: 1px solid var(--fono-border-soft);
  padding-block: var(--fono-space-3);
  font-size: var(--fono-type-sm);
  overflow-wrap: anywhere;
}
.application-context p {
  margin-block: 0 var(--fono-space-2);
}
</style>
