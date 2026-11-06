<script setup lang="ts">
import { computed, onMounted, onScopeDispose, watch } from "vue";
import { WlButton, WlField, WlInput, WlTextarea } from "@whitelife-core/ui-kit";
import type { Preferences } from "../../../shared/domain/contracts";
import type { RemoteServerInfo } from "../../../shared/domain/recognitionSource";
import { defaultRemoteSsh } from "../../../shared/domain/recognitionSource";
import { message as $t } from "../../../shared/domain/messages";
import { useWorkspace } from "../../../shared/application/workspace";
import SelectField from "../../../shared/presentation/SelectField.vue";
import SettingsDisclosure from "../../../shared/presentation/SettingsDisclosure.vue";
import {
  createRecognitionSourceController,
  connectionErrorText,
} from "../application/recognitionSource";

const draft = defineModel<Preferences>({ required: true });
const emit = defineEmits<{ checked: [server: RemoteServerInfo | null] }>();
const workspace = useWorkspace();
const connection = computed(() => draft.value.recognitionSource);
const remote = computed(() => connection.value.source === "fono_server");
const transport = computed<string | number>({
  get: () => (connection.value.ssh ? "ssh" : "direct"),
  set: (value) => {
    if (value === "ssh") {
      connection.value.ssh = defaultRemoteSsh();
      connection.value.url = "http://127.0.0.1:17832";
      connection.value.caPem = "";
    } else delete connection.value.ssh;
  },
});
const controller = createRecognitionSourceController({
  settings: () => connection.value,
  port: workspace.recognition,
  checked: (server) => emit("checked", server),
});
const state = controller.state;
const token = computed({ get: () => state.token, set: controller.setToken });
const locked = computed(() =>
  [
    "listening",
    "silence",
    "transcribing",
    "processing",
    "awaiting_action",
  ].includes(workspace.state.phase),
);
const sourceOptions = computed(() => [
  { value: "local", label: $t("recognitionSource.local") },
  { value: "fono_server", label: $t("recognitionSource.server") },
]);
const fallbackOptions = computed(() => [
  { value: "none", label: $t("recognitionSource.fallbackNone") },
  { value: "ask", label: $t("recognitionSource.fallbackAsk") },
  { value: "local", label: $t("recognitionSource.fallbackLocal") },
]);
const transportOptions = computed(() => [
  { value: "direct", label: $t("recognitionSource.directTransport") },
  { value: "ssh", label: $t("recognitionSource.sshTransport") },
]);
const installed = computed(() =>
  workspace.state.models
    .filter((model) => model.status === "installed")
    .map((model) => ({ value: model.id, label: model.name })),
);
const modelOptions = computed(() => {
  const options =
    state.server?.models
      .filter((model) => model.installed)
      .map((model) => ({ value: model.id, label: model.label })) ?? [];
  if (
    connection.value.model &&
    !options.some((model) => model.value === connection.value.model)
  )
    options.unshift({
      value: connection.value.model,
      label: connection.value.model,
    });
  return options;
});
watch(
  () => [connection.value.source, connection.value.url, connection.value.caPem],
  () => {
    if (connection.value.source === "local" && state.token)
      controller.setToken("");
    else controller.invalidate();
  },
  { flush: "sync" },
);
watch(
  () =>
    JSON.stringify([connection.value.ssh?.host, connection.value.ssh?.port]),
  () => {
    if (connection.value.ssh) connection.value.ssh.hostKey = "";
    controller.invalidate();
  },
  { flush: "sync" },
);
watch(
  () => JSON.stringify(connection.value.ssh ?? null),
  () => controller.invalidate(),
  { flush: "sync" },
);
watch(
  () => connection.value.model,
  () => controller.invalidate(false),
  { flush: "sync" },
);
onMounted(() => void controller.loadStatus());
onScopeDispose(controller.dispose);
</script>
<template>
  <section id="recognition-source" class="form-stack recognition-source">
    <SelectField
      id="recognitionSource"
      v-model="connection.source"
      :label="$t('recognitionSource.title')"
      :options="sourceOptions"
      :disabled="locked"
    />
    <p v-if="locked" class="muted">{{ $t("recognitionSource.locked") }}</p>
    <template v-if="remote">
      <SelectField
        id="remote-server-transport"
        v-model="transport"
        :label="$t('recognitionSource.transport')"
        :options="transportOptions"
        :disabled="locked"
      />
      <div class="form-grid">
        <WlField
          v-slot="field"
          id="remote-server-name"
          :label="$t('recognitionSource.name')"
          ><WlInput
            v-bind="field"
            v-model="connection.displayName"
            :disabled="locked"
            maxlength="80"
            :placeholder="$t('recognitionSource.namePlaceholder')"
        /></WlField>
        <WlField
          v-if="!connection.ssh"
          v-slot="field"
          id="remote-server-url"
          :label="$t('recognitionSource.url')"
          :hint="$t('recognitionSource.urlHint')"
          ><WlInput
            v-bind="field"
            v-model="connection.url"
            :disabled="locked"
            inputmode="url"
            autocomplete="off"
            placeholder="http://127.0.0.1:17833"
        /></WlField>
      </div>
      <div v-if="connection.ssh" class="form-stack">
        <p class="muted">{{ $t("recognitionSource.sshHint") }}</p>
        <div class="form-grid">
          <WlField
            v-slot="field"
            id="remote-ssh-host"
            :label="$t('recognitionSource.sshHost')"
          >
            <WlInput
              v-bind="field"
              v-model="connection.ssh.host"
              :disabled="locked"
              maxlength="253"
              autocomplete="off"
              placeholder="server.example"
            />
          </WlField>
          <WlField
            v-slot="field"
            id="remote-ssh-user"
            :label="$t('recognitionSource.sshUser')"
          >
            <WlInput
              v-bind="field"
              v-model="connection.ssh.user"
              :disabled="locked"
              maxlength="64"
              autocomplete="off"
            />
          </WlField>
        </div>
        <WlField
          v-slot="field"
          id="remote-ssh-identity"
          :label="$t('recognitionSource.sshIdentity')"
          :hint="$t('recognitionSource.sshIdentityHint')"
        >
          <WlInput
            v-bind="field"
            v-model="connection.ssh.identityFile"
            :disabled="locked || state.identityBusy"
            maxlength="4096"
            autocomplete="off"
          />
        </WlField>
        <div class="actions">
          <WlButton
            size="sm"
            variant="ghost"
            :loading="state.identityBusy"
            :disabled="
              locked ||
              !workspace.native ||
              !workspace.recognition ||
              state.identityBusy
            "
            @click="controller.pickIdentityFile"
            >{{ $t("recognitionSource.sshChooseIdentity") }}</WlButton
          >
        </div>
        <SettingsDisclosure
          id="remote-ssh-ports"
          :title="$t('recognitionSource.sshPorts')"
        >
          <div class="form-grid">
            <WlField
              v-slot="field"
              id="remote-ssh-port"
              :label="$t('recognitionSource.sshPort')"
            >
              <WlInput
                v-bind="field"
                :model-value="String(connection.ssh.port)"
                type="number"
                inputmode="numeric"
                min="1"
                max="65535"
                :disabled="locked"
                @update:model-value="connection.ssh.port = Number($event)"
              />
            </WlField>
            <WlField
              v-slot="field"
              id="remote-ssh-server-port"
              :label="$t('recognitionSource.sshServerPort')"
              :hint="$t('recognitionSource.sshServerPortHint')"
            >
              <WlInput
                v-bind="field"
                :model-value="String(connection.ssh.serverPort)"
                type="number"
                inputmode="numeric"
                min="1"
                max="65535"
                :disabled="locked"
                @update:model-value="connection.ssh.serverPort = Number($event)"
              />
            </WlField>
          </div>
        </SettingsDisclosure>
        <div class="actions">
          <WlButton
            size="sm"
            :loading="state.probingHost"
            :disabled="
              locked ||
              !workspace.recognition ||
              !connection.ssh.host ||
              state.probingHost
            "
            @click="controller.probeSshHost"
            >{{ $t("recognitionSource.sshProbeHost") }}</WlButton
          >
          <small v-if="connection.ssh.hostKey">{{
            $t("recognitionSource.sshTrusted")
          }}</small>
        </div>
        <div
          v-if="state.hostKeyCandidate"
          class="form-stack notice"
          role="status"
        >
          <strong>{{ $t("recognitionSource.sshFingerprint") }}</strong>
          <code class="ssh-fingerprint">{{
            state.hostKeyCandidate.fingerprint
          }}</code>
          <p>{{ $t("recognitionSource.sshTrustHint") }}</p>
          <WlButton
            size="sm"
            :disabled="locked"
            @click="controller.trustHostKey"
            >{{ $t("recognitionSource.sshTrustHost") }}</WlButton
          >
        </div>
        <p v-else-if="!connection.ssh.hostKey" class="muted">
          {{ $t("recognitionSource.sshHostKeyRequired") }}
        </p>
        <p v-if="state.sshError" class="error-text" role="alert">
          {{ state.sshError }}
        </p>
      </div>
      <p class="notice">
        {{
          $t("recognitionSource.privacy", {
            server:
              connection.displayName.trim() || $t("recognitionSource.unnamed"),
          })
        }}
      </p>
      <small v-if="!workspace.native" class="muted">{{
        $t("recognitionSource.demo")
      }}</small>
      <WlField
        v-slot="field"
        id="remote-server-token"
        :label="$t('recognitionSource.token')"
        :hint="$t('recognitionSource.tokenHint')"
        ><WlInput
          v-bind="field"
          v-model="token"
          type="password"
          autocomplete="new-password"
          :disabled="locked || state.tokenBusy"
          maxlength="4096"
          :placeholder="
            state.hasToken ? $t('recognitionSource.tokenStored') : ''
          "
      /></WlField>
      <div class="actions">
        <WlButton
          size="sm"
          :disabled="
            !workspace.recognition || locked || !token || state.tokenBusy
          "
          :loading="state.tokenBusy"
          @click="controller.saveToken"
          >{{ $t("recognitionSource.saveToken") }}</WlButton
        >
        <WlButton
          size="sm"
          variant="ghost"
          :disabled="
            !workspace.recognition ||
            locked ||
            !state.hasToken ||
            state.tokenBusy
          "
          @click="controller.clearToken"
          >{{ $t("recognitionSource.deleteToken") }}</WlButton
        >
        <small>{{
          state.hasToken === null
            ? $t("recognitionSource.tokenUnknown")
            : state.hasToken
              ? $t("recognitionSource.tokenStored")
              : $t("recognitionSource.tokenMissing")
        }}</small>
      </div>
      <p v-if="state.tokenNotice" class="muted" role="status">
        {{
          state.tokenNotice === "saved"
            ? $t("recognitionSource.tokenSaved")
            : $t("recognitionSource.tokenDeleted")
        }}
      </p>
      <p v-if="state.tokenError" class="error-text" role="alert">
        {{ state.tokenError }}
      </p>
      <SettingsDisclosure
        v-if="!connection.ssh"
        id="remote-server-ca"
        :title="$t('recognitionSource.ca')"
        ><WlField
          v-slot="field"
          :label="$t('recognitionSource.ca')"
          :hint="$t('recognitionSource.caHint')"
          ><WlTextarea
            v-bind="field"
            v-model="connection.caPem"
            :disabled="locked"
            :rows="4"
            maxlength="262144"
            spellcheck="false" /></WlField
      ></SettingsDisclosure>
      <div class="actions">
        <WlButton
          size="sm"
          :loading="state.checking"
          :disabled="
            !workspace.recognition ||
            locked ||
            !connection.url.trim() ||
            state.tokenBusy ||
            (connection.ssh && !connection.ssh.hostKey)
          "
          @click="controller.check"
          >{{ $t("recognitionSource.check") }}</WlButton
        >
      </div>
      <p v-if="state.checking" class="muted" role="status">
        {{ $t("recognitionSource.checking") }}
      </p>
      <p v-if="state.result?.kind === 'error'" class="error-text" role="alert">
        {{ connectionErrorText(state.result.error) }}
      </p>
      <div v-if="state.server" class="remote-server-summary" role="status">
        <strong
          >{{ $t("recognitionSource.checked") }} · Fono
          {{ state.server.version }} · API {{ state.server.apiVersion }}</strong
        >
        <p>
          {{ $t("recognitionSource.state") }}:
          {{ $t(`recognitionSource.state.${state.server.state}`) }}
        </p>
        <p v-for="device in state.server.devices" :key="device.id">
          {{ device.name }} · {{ device.backend.toUpperCase()
          }}{{
            device.available
              ? ""
              : " · " + $t("recognitionSource.deviceUnavailable")
          }}
        </p>
        <small>{{ $t("recognitionSource.observation") }}</small>
      </div>
      <SelectField
        id="remote-server-model"
        v-model="connection.model"
        :label="$t('recognitionSource.model')"
        :options="modelOptions"
        :disabled="locked || !state.server"
        :hint="$t('recognitionSource.modelHint')"
      />
      <SelectField
        id="remote-server-fallback"
        v-model="connection.fallback"
        :label="$t('recognitionSource.fallback')"
        :options="fallbackOptions"
        :disabled="locked"
      />
      <SelectField
        v-if="connection.fallback === 'local'"
        id="remote-server-fallback-model"
        v-model="connection.fallbackModel"
        :label="$t('recognitionSource.fallbackModel')"
        :options="installed"
        :disabled="locked"
        :hint="$t('recognitionSource.fallbackHint')"
      />
      <small v-else class="muted">{{
        $t("recognitionSource.manualFallback")
      }}</small>
      <small class="muted">{{ $t("recognitionSource.limits") }}</small>
    </template>
  </section>
</template>
<style scoped>
.ssh-fingerprint {
  overflow-wrap: anywhere;
}
.remote-server-summary {
  overflow-wrap: anywhere;
  padding: var(--fono-space-3);
  border: 1px solid var(--fono-border);
  border-radius: var(--fono-radius-sm);
  color: var(--fono-secondary);
  font-size: var(--fono-type-sm);
}
.remote-server-summary p {
  margin-block: var(--fono-space-2);
}
</style>
