<script setup lang="ts">
import { message as $t } from "../../../shared/domain/messages";

import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import { useVersionHistory } from "../application/useVersionHistory";
import VersionNote from "./VersionNote.vue";
const workspace = useWorkspace();
const { current, expanded, recent, hiddenCount, latest, previous } =
  useVersionHistory(() => workspace.updates.state.currentVersion);
</script>
<template>
  <div class="page releases-page">
    <PageHeading :title="$t('ui.d9e4291457')" :description="$t('ui.05bd7978a3')"
      ><RouterLink class="text-link" to="/updates"
        ><AppIcon name="download" :size="15" />{{
          $t("ui.455471db0c")
        }}</RouterLink
      ></PageHeading
    >
    <section class="releases-current" :aria-label="$t('ui.480fe74477')">
      <AppIcon name="check" :size="20" />
      <div>
        <small>{{
          workspace.native ? $t("ui.319b48dfe2") : $t("ui.a1dfa2ea75")
        }}</small
        ><strong>Fono {{ current || "—" }}</strong>
      </div>
      <p>{{ $t("ui.d1c49de751") }}</p>
    </section>
    <section class="releases-series" aria-labelledby="recent-versions-heading">
      <header>
        <div>
          <h2 id="recent-versions-heading">{{ latest.title }}</h2>
          <p>{{ latest.description }}</p>
        </div>
        <small>{{ latest.entries.length }} {{ $t("ui.7bfceb34a4") }}</small>
      </header>
      <div id="recent-version-list" class="version-note-list">
        <VersionNote
          v-for="entry in recent"
          :key="entry.version"
          :entry="entry"
          :current="entry.version === current"
        />
      </div>
      <WlButton
        class="releases-expand"
        variant="ghost"
        size="sm"
        :aria-expanded="expanded"
        aria-controls="recent-version-list"
        @click="expanded = !expanded"
        ><AppIcon
          :name="expanded ? 'chevron-up' : 'chevron-down'"
          :size="16"
        />{{
          expanded
            ? $t("ui.9134c7b635")
            : $t("ui.cce44b719b", { v0: String(hiddenCount) })
        }}</WlButton
      >
    </section>
    <details class="releases-previous">
      <summary>
        <div>
          <strong>{{ previous.title }}</strong
          ><small>{{ previous.description }}</small>
        </div>
        <span>{{ previous.entries.length }} {{ $t("ui.1fa9141302") }}</span
        ><AppIcon name="chevron-down" :size="18" />
      </summary>
      <div class="version-note-list">
        <VersionNote
          v-for="entry in previous.entries"
          :key="entry.version"
          :entry="entry"
          :current="entry.version === current"
        />
      </div>
    </details>
  </div>
</template>
