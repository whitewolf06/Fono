<script setup lang="ts">
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
    <PageHeading
      title="История версий"
      description="Что менялось в Fono: от нового интерфейса до настроек обработки текста."
      ><RouterLink class="text-link" to="/updates"
        ><AppIcon name="download" :size="15" />Обновления</RouterLink
      ></PageHeading
    >
    <section class="releases-current" aria-label="Версия этого приложения">
      <AppIcon name="check" :size="20" />
      <div>
        <small>{{
          workspace.native
            ? "Установленная версия"
            : "Версия браузерного примера"
        }}</small
        ><strong>Fono {{ current || "—" }}</strong>
      </div>
      <p>
        Даты и изменения восстановлены по истории проекта. Здесь также показаны
        промежуточные версии разработки.
      </p>
    </section>
    <section class="releases-series" aria-labelledby="recent-versions-heading">
      <header>
        <div>
          <h2 id="recent-versions-heading">{{ latest.title }}</h2>
          <p>{{ latest.description }}</p>
        </div>
        <small>{{ latest.entries.length }} версий</small>
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
            ? "Свернуть ранние версии"
            : `Показать ещё ${hiddenCount} версий`
        }}</WlButton
      >
    </section>
    <details class="releases-previous">
      <summary>
        <div>
          <strong>{{ previous.title }}</strong
          ><small>{{ previous.description }}</small>
        </div>
        <span>{{ previous.entries.length }} версии</span
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
