<script setup lang="ts">
import { computed, reactive, watch, onBeforeUnmount } from "vue";
import { WlButton, WlCard } from "@whitelife-core/ui-kit";
import type { Dictation } from "../../../shared/domain/contracts";
import { message as $t } from "../../../shared/domain/messages";
import { useWorkspace } from "../../../shared/application/workspace";
import { useInteraction } from "../../../shared/application/interaction";
import { useFeedback } from "../../../shared/application/feedback";
import SelectField from "../../../shared/presentation/SelectField.vue";
import { rewriteStyles } from "../domain/rewrite";
import {
  canRewrite,
  createRewriteController,
  initialRewriteState,
  matchesRewrite,
  selectedRewrite,
  selectedRewriteProfile,
} from "../application/rewriteController";
import { exactDateLabel } from "../application/statistics";

const props = defineProps<{ entry: Dictation }>();
const workspace = useWorkspace();
const interaction = useInteraction();
const feedback = useFeedback();
const state = reactive(initialRewriteState());
const controller = createRewriteController(workspace.historyRewrite, state);
const result = computed(() => selectedRewrite(state));
const profile = computed(() => selectedRewriteProfile(state));
const blocked = computed(
  () => state.generating || state.loading || state.removing,
);
const matches = computed(
  () => result.value && matchesRewrite(state, result.value),
);
const cached = computed(() =>
  state.context?.variants.some((v) => matchesRewrite(state, v)),
);
const profiles = computed(
  () =>
    state.context?.profiles.map((p) => ({
      value: p.id,
      label: `${p.name} · ${p.model || $t("historyRewrite.noModel")}`,
    })) || [],
);
const sources = computed(() => [
  { value: "result", label: $t("historyRewrite.result") },
  ...(state.context?.originalFingerprint
    ? [{ value: "original", label: $t("historyRewrite.original") }]
    : []),
]);
const styles = computed(() =>
  rewriteStyles.map((style) => ({
    value: style,
    label: $t(`historyRewrite.style.${style}`),
  })),
);
const saved = computed(
  () =>
    state.context?.variants.map((v) => ({
      value: v.id,
      label: `${$t(`historyRewrite.style.${v.style}`)} · ${v.profileName} · ${exactDateLabel(v.createdAt)}`,
    })) || [],
);
watch(
  () =>
    JSON.stringify([
      props.entry.id,
      props.entry.text,
      props.entry.original,
      workspace.state.preferences.historyEnabled,
      workspace.state.preferences.profile,
      workspace.state.preferences.processingModel,
      workspace.state.profiles,
    ]),
  () => {
    void controller.open(props.entry.id);
  },
  { immediate: true },
);
onBeforeUnmount(controller.dispose);
async function generate() {
  await controller.generate((p) =>
    interaction.confirm({
      title: $t("historyRewrite.cloudTitle"),
      text: $t("historyRewrite.cloudBody", { profile: p.name, model: p.model }),
      accept: $t("historyRewrite.send"),
      cancel: $t("historyRewrite.cancel"),
    }),
  );
}
async function remove() {
  const entryId = state.entryId;
  const variantId = state.selectedId;
  if (
    await interaction.confirm({
      title: $t("historyRewrite.deleteTitle"),
      text: $t("historyRewrite.deleteBody"),
      accept: $t("historyRewrite.delete"),
      danger: true,
    })
  )
    await controller.remove(entryId, variantId);
}
</script>

<template>
  <section
    class="history-rewrite"
    :aria-label="$t('historyRewrite.title')"
    data-testid="history-rewrite"
  >
    <WlCard class="rewrite-card">
      <div class="rewrite-content">
        <h3>{{ $t("historyRewrite.title") }}</h3>
        <p class="muted">{{ $t("historyRewrite.description") }}</p>
        <p v-if="!workspace.native" class="notice">
          {{ $t("historyRewrite.demo") }}
        </p>
        <p v-if="state.loading" role="status" class="muted">
          {{ $t("historyRewrite.loading") }}
        </p>
        <template v-if="state.context">
          <p v-if="!state.context.enabled" class="notice">
            {{ $t("historyRewrite.historyDisabled") }}
          </p>
          <p v-if="!state.context.profiles.length" class="notice">
            {{ $t("historyRewrite.noProfile") }}
          </p>
          <div class="rewrite-fields">
            <SelectField
              v-model="state.source"
              :label="$t('historyRewrite.source')"
              :options="sources"
              :disabled="blocked"
            />
            <SelectField
              v-model="state.style"
              :label="$t('historyRewrite.style')"
              :options="styles"
              :disabled="blocked"
            />
            <SelectField
              v-model="state.profileId"
              :label="$t('historyRewrite.profile')"
              :options="profiles"
              :disabled="blocked || !profiles.length"
            />
          </div>
          <p v-if="profile && !profile.configured" class="notice">
            {{ $t("historyRewrite.incomplete") }}
          </p>
          <p v-if="profile?.cloud" class="notice">
            {{ $t("historyRewrite.cloudNotice") }}
          </p>
          <p class="muted">{{ $t("historyRewrite.explicitOnly") }}</p>
          <RouterLink class="text-link" to="/settings/processing">{{
            $t("historyRewrite.settings")
          }}</RouterLink>
          <div class="actions">
            <WlButton
              size="sm"
              variant="primary"
              :loading="state.generating"
              :disabled="!canRewrite(state)"
              @click="generate"
              >{{
                cached
                  ? $t("historyRewrite.showSaved")
                  : $t("historyRewrite.generate")
              }}</WlButton
            >
            <WlButton
              v-if="state.generating"
              size="sm"
              :disabled="state.cancelling"
              @click="controller.cancel"
              >{{
                state.cancelling
                  ? $t("historyRewrite.cancelling")
                  : $t("historyRewrite.cancel")
              }}</WlButton
            >
          </div>
          <p v-if="state.generating" role="status" class="muted">
            {{ $t("historyRewrite.waiting") }}
          </p>
          <SelectField
            v-if="saved.length"
            v-model="state.selectedId"
            :label="$t('historyRewrite.saved')"
            :options="saved"
            :disabled="blocked"
          />
          <article
            v-if="result"
            class="rewrite-result"
            data-testid="history-rewrite-result"
          >
            <h4>{{ $t("historyRewrite.proposal") }}</h4>
            <p class="muted rewrite-provenance">
              {{ $t(`historyRewrite.style.${result.style}`) }} ·
              {{ result.profileName }} · {{ result.model }} ·
              {{ exactDateLabel(result.createdAt) }}
            </p>
            <p class="muted">
              {{ $t("historyRewrite.basedOn") }}
              {{
                result.source === "original"
                  ? $t("historyRewrite.original")
                  : $t("historyRewrite.result")
              }}
            </p>
            <p v-if="result.stale" class="notice" role="status">
              {{ $t("historyRewrite.stale") }}
            </p>
            <p v-else-if="!matches" class="muted">
              {{ $t("historyRewrite.otherSettings") }}
            </p>
            <div class="reading-text" tabindex="0">{{ result.text }}</div>
            <p class="muted">{{ $t("historyRewrite.separate") }}</p>
            <div class="actions">
              <WlButton
                size="sm"
                :disabled="feedback.busy.value"
                @click="feedback.run(() => workspace.copy(result!.text))"
                >{{ $t("historyRewrite.copy") }}</WlButton
              >
              <WlButton
                size="sm"
                variant="danger-quiet"
                :disabled="blocked"
                @click="remove"
                >{{ $t("historyRewrite.delete") }}</WlButton
              >
            </div>
          </article>
        </template>
        <p
          v-if="state.error || feedback.error.value"
          class="error-text"
          role="alert"
        >
          {{ state.error || feedback.error.value }}
        </p>
        <WlButton
          v-if="state.error && !blocked"
          size="sm"
          variant="ghost"
          @click="controller.refresh"
          >{{ $t("historyRewrite.refresh") }}</WlButton
        >
      </div>
    </WlCard>
  </section>
</template>

<style scoped>
.history-rewrite {
  min-width: 0;
  margin-top: var(--fono-space-4);
}
.rewrite-content,
.rewrite-fields,
.rewrite-result {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: var(--fono-space-3);
  min-width: 0;
}
.rewrite-result {
  border-top: 1px solid var(--fono-border);
  padding-top: var(--fono-space-3);
}
.rewrite-provenance {
  font-size: var(--fono-type-sm);
  overflow-wrap: anywhere;
}
.rewrite-card :deep(.wl-field),
.rewrite-card :deep(.wl-select),
.rewrite-card :deep(.wl-card__body) {
  min-width: 0;
  max-width: 100%;
}
.actions {
  flex-wrap: wrap;
}
</style>
