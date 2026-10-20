<script setup lang="ts">
import { computed, ref } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import type { ApplicationProfile } from "../../../shared/domain/applicationProfiles";
import { newApplicationProfile } from "../../../shared/domain/applicationProfiles";
import ApplicationProfileEditor from "./ApplicationProfileEditor.vue";
import { useWorkspace } from "../../../shared/application/workspace";
const draft = defineModel<Preferences>({ required: true });
const workspace = useWorkspace();
const editing = ref<ApplicationProfile | null>(null);
const others = computed(() =>
  draft.value.applicationProfiles.filter((p) => p.id !== editing.value?.id),
);
function store(profile: ApplicationProfile) {
  const index = draft.value.applicationProfiles.findIndex(
    (p) => p.id === profile.id,
  );
  const next = [...draft.value.applicationProfiles];
  if (index < 0) next.push(profile);
  else next[index] = profile;
  draft.value.applicationProfiles = next;
  editing.value = null;
}
function toggle(profile: ApplicationProfile) {
  draft.value.applicationProfiles = draft.value.applicationProfiles.map((p) =>
    p.id === profile.id ? { ...p, enabled: !p.enabled } : p,
  );
}
</script>
<template>
  <section id="application-profiles" class="form-stack">
    <div class="section-divider" />
    <h3>Профили приложений</h3>
    <p class="muted">
      Стиль, перевод, язык распознавания и вставка для нужного приложения.
      Профиль выбирается при старте диктовки; ручной выбор в индикаторе важнее.
      Для остальных приложений действуют общие настройки.
    </p>
    <p class="muted">
      Fono использует только имя exe исходного приложения. Заголовки, адреса
      страниц и содержимое окон для профиля не читаются. Выбор не переносится
      вслед за фокусом.
    </p>
    <p class="muted">
      Изменения профилей применяются после «Сохранить» и со следующей диктовки.
    </p>
    <p v-if="!workspace.native" class="notice">
      Браузерный макет: профили живут до перезагрузки. Приложения компьютера не
      определяются.
    </p>
    <ul
      v-if="draft.applicationProfiles.length"
      class="dictionary-list"
      aria-label="Профили приложений"
    >
      <li v-for="profile in draft.applicationProfiles" :key="profile.id">
        <div>
          <strong>{{ profile.name }}</strong
          ><small
            >{{ profile.executables.join(", ") }} ·
            {{ profile.enabled ? "Включён" : "Выключен" }}</small
          >
        </div>
        <div class="actions">
          <WlButton
            size="sm"
            variant="ghost"
            :aria-label="'Изменить профиль ' + profile.name"
            @click="editing = profile"
            >Изменить</WlButton
          ><WlButton
            size="sm"
            variant="ghost"
            :aria-label="'Переключить профиль ' + profile.name"
            @click="toggle(profile)"
            >{{ profile.enabled ? "Выключить" : "Включить" }}</WlButton
          ><WlButton
            size="sm"
            variant="ghost"
            :aria-label="'Удалить профиль ' + profile.name"
            @click="
              draft.applicationProfiles = draft.applicationProfiles.filter(
                (p) => p.id !== profile.id,
              )
            "
            >Удалить</WlButton
          >
        </div>
      </li>
    </ul>
    <p v-else class="muted">
      Профилей пока нет — используются общие настройки.
    </p>
    <div class="actions">
      <WlButton
        :disabled="draft.applicationProfiles.length >= 32"
        @click="editing = newApplicationProfile(draft.applicationProfiles)"
        >Добавить профиль приложения</WlButton
      ><small class="muted">{{ draft.applicationProfiles.length }} / 32</small>
    </div>
    <ApplicationProfileEditor
      v-if="editing"
      :profile="editing"
      :others="others"
      @close="editing = null"
      @save="store"
    />
  </section>
</template>
