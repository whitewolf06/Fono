<script setup lang="ts">
import { computed, ref } from "vue";
import { useRoute } from "vue-router";
import { WlButton, WlDialog } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import PageHeading from "../../../shared/presentation/PageHeading.vue";
import AppIcon from "../../../shared/presentation/AppIcon.vue";
import StatusDot from "../../../shared/presentation/StatusDot.vue";
import EmptyState from "../../../shared/presentation/EmptyState.vue";
import { PreferenceToggle } from "../../preferences";
import ConnectionPanel from "./ConnectionPanel.vue";
const route = useRoute();
const workspace = useWorkspace();
const { run } = useFeedback();
const selectedId = ref<string | null>(null);
const selected = computed(() =>
  workspace.state.jobs.find((j) => j.id === selectedId.value),
);
const visible = computed({
  get: () => !!selected.value,
  set: () => {
    selectedId.value = null;
  },
});
const jobs = computed(() => workspace.state.jobs);
const queue = computed(() =>
  jobs.value.filter((j) => ["queued", "running"].includes(j.state)),
);
const labels = {
  queued: "В очереди",
  running: "Распознавание",
  done: "Готово",
  error: "Ошибка",
  cancelled: "Отменена",
};
</script>
<template>
  <div class="page">
    <PageHeading
      title="API-сервис"
      description="Fono как голосовой инструмент для других приложений."
      ><PreferenceToggle name="serviceEnabled" label="API-сервис" compact
    /></PageHeading>
    <div class="service-summary">
      <div>
        <StatusDot
          :health="workspace.state.preferences.serviceEnabled ? 'ready' : 'off'"
          :label="
            workspace.state.preferences.serviceEnabled
              ? 'Сервис включён'
              : 'Сервис выключен'
          "
        /><code>{{ workspace.state.serviceAddress || "127.0.0.1:17832" }}</code>
      </div>
      <span>{{ queue.length }} / 4 в очереди</span>
    </div>
    <p v-if="workspace.state.serviceError" class="notice error" role="alert">
      {{ workspace.state.serviceError }}
    </p>
    <nav class="page-tabs" aria-label="Раздел API">
      <RouterLink
        to="/api/tasks"
        :class="{ selected: route.params.tab !== 'connection' }"
        >Задачи</RouterLink
      ><RouterLink
        to="/api/connection"
        :class="{ selected: route.params.tab === 'connection' }"
        >Подключение</RouterLink
      >
    </nav>
    <ConnectionPanel v-if="route.params.tab === 'connection'" />
    <template v-else>
      <div class="section-header">
        <h2>Задачи распознавания</h2>
        <WlButton
          size="sm"
          v-if="!workspace.native"
          :disabled="!workspace.state.preferences.serviceEnabled"
          @click="
            run(
              () => workspace.service.enqueue(),
              'Демонстрационная задача добавлена',
            )
          "
          ><template #icon><AppIcon name="plus" /></template>Тестовая
          задача</WlButton
        >
      </div>
      <p v-if="!workspace.state.preferences.serviceEnabled" class="notice">
        Включите сервис, чтобы принимать задачи. История результатов доступна и
        при выключенном сервисе.
      </p>
      <p v-if="queue.length >= 4" class="notice">
        Очередь заполнена. Можно отменить ожидающую задачу. В сценарии проверки
        очередь остаётся заполненной.
      </p>
      <div v-if="jobs.length" class="job-list">
        <article v-for="job in jobs" :key="job.id" class="job-row">
          <span class="job-icon"
            ><AppIcon
              :name="
                job.state === 'error'
                  ? 'warn'
                  : job.state === 'done'
                    ? 'check'
                    : 'file'
              "
          /></span>
          <div class="job-copy">
            <button class="job-name" @click="selectedId = job.id">
              {{ job.name }}
            </button>
            <p v-if="job.error" class="error-text">{{ job.error }}</p>
            <small v-else>{{ job.id }} · {{ job.seconds }} сек</small>
          </div>
          <StatusDot
            :health="
              job.state === 'error'
                ? 'error'
                : job.state === 'done'
                  ? 'ready'
                  : job.state === 'cancelled'
                    ? 'off'
                    : 'loading'
            "
            :label="labels[job.state]"
          /><WlButton
            v-if="job.state === 'queued' || job.state === 'running'"
            size="sm"
            variant="ghost"
            @click="run(() => workspace.service.cancel(job.id))"
            >Отменить</WlButton
          ><WlButton
            v-else-if="job.text"
            size="sm"
            variant="ghost"
            :aria-label="'Копировать результат ' + job.name"
            @click="
              run(() => workspace.copy(job.text || ''), 'Результат скопирован')
            "
            ><template #icon><AppIcon name="copy" /></template></WlButton
          ><WlButton
            v-else-if="!workspace.native && job.state === 'error'"
            size="sm"
            variant="ghost"
            :disabled="!workspace.state.preferences.serviceEnabled"
            @click="run(() => workspace.service.retry(job.id))"
            >Повторить</WlButton
          >
        </article>
      </div>
      <EmptyState
        v-else
        title="Очередь свободна"
        :text="
          workspace.native
            ? 'Задачи от приложений появятся здесь. Инструкция — на вкладке «Подключение».'
            : 'Для проверки можно добавить демонстрационную задачу.'
        "
      />
    </template>
    <WlDialog v-model:visible="visible" :header="selected?.name" width="560px"
      ><template v-if="selected"
        ><StatusDot
          :health="
            selected.state === 'error'
              ? 'error'
              : selected.state === 'done'
                ? 'ready'
                : 'loading'
          "
          :label="labels[selected.state]"
        />
        <p class="reading-text">
          {{
            selected.text ||
            selected.error ||
            "Результат появится после завершения задачи."
          }}
        </p></template
      ><template #footer
        ><WlButton
          v-if="selected?.text"
          @click="
            run(
              () => workspace.copy(selected?.text || ''),
              'Результат скопирован',
            )
          "
          >Копировать результат</WlButton
        ><WlButton @click="selectedId = null">Закрыть</WlButton></template
      ></WlDialog
    >
  </div>
</template>
