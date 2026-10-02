<script setup lang="ts">
import { computed } from "vue";
import { WlButton } from "@whitelife-core/ui-kit";
import { useWorkspace } from "../../../shared/application/workspace";
import { useFeedback } from "../../../shared/application/feedback";
import PreferenceToggle from "./PreferenceToggle.vue";
import StatusDot from "../../../shared/presentation/StatusDot.vue";
const workspace = useWorkspace();
const { run, busy } = useFeedback();
const checks = computed(() => workspace.diagnostics());
const build = __FONO_FRONTEND_BUILD__;
async function checkAll() {
  await new Promise<void>((resolve) => setTimeout(resolve, 500));
  workspace.state.logs.unshift(
    "Проверены демонстрационные состояния компонентов",
  );
}
</script>
<template>
  <div class="form-stack">
    <div class="section-header">
      <h3>Компоненты</h3>
      <WlButton
        size="sm"
        :loading="busy"
        @click="run(checkAll, 'Проверка завершена')"
        >Проверить всё</WlButton
      >
    </div>
    <div v-for="check in checks" :key="check.id" class="diagnostic-row">
      <div>
        <strong>{{ check.title }}</strong>
        <p>{{ check.detail }}</p>
      </div>
      <StatusDot :health="check.health" /><RouterLink
        v-if="check.health !== 'ready'"
        class="text-link"
        :to="
          check.id === 'service' ? '/api/tasks' : '/settings/' + check.section
        "
        >Настроить</RouterLink
      >
    </div>
    <PreferenceToggle
      name="verboseLogging"
      label="Подробный журнал"
      description="Технические события для поиска проблемы."
    />
    <section id="logs">
      <div class="section-header">
        <h3>Журнал</h3>
        <WlButton
          size="sm"
          @click="
            run(
              () => workspace.copy(workspace.state.logs.join('\n')),
              'Журнал скопирован',
            )
          "
          >Копировать</WlButton
        >
      </div>
      <pre class="log-view">{{
        workspace.state.logs.slice(0, 20).join("\n")
      }}</pre>
    </section>
    <div class="technical-info">
      <span>Fono {{ build.version }}</span
      ><span>Vue 3 · WhiteUI 0.6.0</span><code>{{ build.revision }}</code
      ><span>Browser mock runtime</span>
    </div>
  </div>
</template>
