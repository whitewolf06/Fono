import { reactive, watch, type InjectionKey, inject } from "vue";
import {
  apiBaseUrl,
  clientEnvironment,
  operationsFrom,
  validClientName,
  type ApiOperation,
  type ClientRequest,
  type PanelAudio,
  type PanelPort,
  type PanelRequest,
  type PanelResponse,
  type PanelTab,
  type ServerCapabilities,
  type ServerHealth,
  type ServerModel,
  type ServerClient,
} from "../domain/panel";

export function createServerPanel(port: PanelPort) {
  const state = reactive({
    authenticated: false,
    signingIn: false,
    refreshing: false,
    autoRefresh: true,
    tab: "status" as PanelTab,
    capabilities: null as ServerCapabilities | null,
    health: null as ServerHealth | null,
    models: [] as ServerModel[],
    operations: [] as ApiOperation[],
    specification: "",
    error: "",
    updatedAt: 0,
    retryAt: 0,
    requestBusy: false,
    requestError: "",
    response: null as PanelResponse | null,
    audio: null as PanelAudio | null,
    connection: port.connection,
    clients: [] as ServerClient[],
    clientsAllowed: null as boolean | null,
    clientsBusy: false,
    clientsLoaded: false,
    clientUrl: "",
    clientName: "",
    clientError: "",
    clientNotice: "",
    clientEnv: "",
    issuedClient: "",
    revokeCandidate: "",
  });
  let token = "",
    generation = 0,
    requestGeneration = 0,
    disposed = false;
  let stopTimer = () => {};
  const controllers = new Set<AbortController>();
  let requestController: AbortController | null = null;
  let clientGeneration = 0;
  let clientController: AbortController | null = null;

  function clearClientSecret() {
    state.clientEnv = "";
    state.issuedClient = "";
    state.clientNotice = "";
  }
  function leaveClients() {
    clientGeneration++;
    clientController?.abort();
    clientController = null;
    state.clientsBusy = false;
    state.revokeCandidate = "";
    clearClientSecret();
  }

  function clearSession() {
    generation++;
    requestGeneration++;
    token = "";
    stopTimer();
    for (const controller of controllers) controller.abort();
    controllers.clear();
    requestController = null;
    leaveClients();
    Object.assign(state, {
      authenticated: false,
      signingIn: false,
      refreshing: false,
      capabilities: null,
      health: null,
      models: [],
      operations: [],
      specification: "",
      error: "",
      retryAt: 0,
      updatedAt: 0,
      requestBusy: false,
      requestError: "",
      response: null,
      audio: null,
      tab: "status",
      clients: [],
      clientsAllowed: null,
      clientsLoaded: false,
      clientUrl: "",
      clientName: "",
      clientError: "",
    });
  }
  function current(id: number) {
    return !disposed && id === generation;
  }
  async function send(request: PanelRequest, signal?: AbortController) {
    const controller = signal ?? new AbortController();
    controllers.add(controller);
    try {
      return await port.request(
        request,
        token,
        controller.signal,
        state.audio ?? undefined,
      );
    } finally {
      controllers.delete(controller);
    }
  }
  function inspect(response: PanelResponse): boolean {
    if (response.status === 401) {
      clearSession();
      state.error = "Токен недействителен или отозван. Войдите снова.";
      return false;
    }
    if (response.status === 429) {
      state.retryAt = port.now() + Math.max(1, response.retryAfter) * 1000;
      state.error = `Сервер ограничил запросы. Повторите через ${Math.max(1, response.retryAfter)} с.`;
      return false;
    }
    return true;
  }
  function schedule() {
    stopTimer();
    if (disposed || !state.authenticated || !state.autoRefresh) return;
    stopTimer = port.schedule(
      () => void refresh(),
      Math.max(10000, state.retryAt - port.now()),
    );
  }
  async function refresh(full = false) {
    if (disposed || !state.authenticated || state.refreshing) return;
    if (state.retryAt > port.now()) {
      schedule();
      return;
    }
    const id = generation;
    state.refreshing = true;
    stopTimer();
    try {
      const health = await send({ method: "GET", path: "/v1/health" });
      if (!current(id) || !inspect(health)) return;
      if (![200, 503].includes(health.status)) throw new Error();
      state.health = health.data as ServerHealth;
      if (full || !state.updatedAt || port.now() - state.updatedAt >= 30000) {
        const models = await send({ method: "GET", path: "/v1/models" });
        if (!current(id) || !inspect(models)) return;
        if (models.status !== 200) throw new Error();
        state.models = (models.data as { data: ServerModel[] }).data;
        state.updatedAt = port.now();
      }
      state.error = "";
      state.retryAt = 0;
    } catch {
      if (current(id))
        state.error =
          "Не удалось обновить статус. Показаны последние полученные данные.";
    } finally {
      if (current(id)) {
        state.refreshing = false;
        schedule();
      }
    }
  }
  async function login(value: string) {
    if (disposed || state.signingIn) return;
    clearSession();
    const candidate = value.trim();
    if (!/^[\x21-\x7e]{1,4096}$/.test(candidate)) {
      state.error = "Введите токен сервера без пробелов и переносов строки.";
      return;
    }
    token = candidate;
    state.signingIn = true;
    const id = generation;
    try {
      const capabilities = await send({
        method: "GET",
        path: "/v1/capabilities",
      });
      if (!current(id) || !inspect(capabilities)) return;
      if (
        capabilities.status !== 200 ||
        (capabilities.data as ServerCapabilities)?.api_version !== 2
      ) {
        state.error =
          "Этот сервер не поддерживает API v2. Проверьте версию Fono Server.";
        token = "";
        return;
      }
      state.capabilities = capabilities.data as ServerCapabilities;
      state.authenticated = true;
      await refresh(true);
      if (!current(id) || !state.authenticated || state.retryAt > port.now())
        return;
      const docs = await send({ method: "GET", path: "/openapi.json" });
      if (!current(id) || !inspect(docs)) return;
      if (docs.status === 200) {
        state.operations = operationsFrom(docs.data);
        state.specification = JSON.stringify(docs.data, null, 2);
      } else
        state.error =
          "Документация временно недоступна. Статус сервера доступен.";
    } catch {
      if (current(id)) {
        if (!state.authenticated) token = "";
        state.error =
          "Нет соединения с сервером. Проверьте адрес и подключение.";
      }
    } finally {
      if (current(id)) {
        state.signingIn = false;
        schedule();
      }
    }
  }
  async function chooseAudio() {
    const id = generation;
    let audio: PanelAudio | null;
    try {
      audio = await port.chooseAudio();
    } catch {
      if (current(id))
        state.requestError = "Не удалось выбрать файл. Попробуйте снова.";
      return;
    }
    if (!current(id) || !state.authenticated || !audio) return;
    state.audio = null;
    state.response = null;
    state.requestError = "";
    if (
      audio.size <= 0 ||
      audio.size > (state.capabilities?.max_upload_bytes ?? 0)
    ) {
      state.requestError = "Файл пустой или превышает ограничение сервера.";
      return;
    }
    state.audio = audio;
  }
  async function run(request: PanelRequest) {
    if (!state.authenticated || state.requestBusy || disposed) return;
    if (state.retryAt > port.now()) {
      state.requestError = "Дождитесь восстановления лимита запросов.";
      return;
    }
    const id = generation,
      operation = ++requestGeneration;
    const controller = new AbortController();
    requestController = controller;
    state.requestBusy = true;
    state.response = null;
    state.requestError = "";
    try {
      const response = await send(request, controller);
      if (!current(id) || operation !== requestGeneration) return;
      const accepted = inspect(response);
      if (!accepted && response.status === 401) return;
      state.response = response;
    } catch {
      if (current(id) && operation === requestGeneration)
        state.requestError =
          "Запрос не завершён. Проверьте соединение; повторный запрос отправляется только кнопкой.";
    } finally {
      if (current(id) && operation === requestGeneration) {
        state.requestBusy = false;
        requestController = null;
      }
    }
  }
  function cancelRequest() {
    requestGeneration++;
    requestController?.abort();
    requestController = null;
    state.requestBusy = false;
    state.requestError =
      "Ожидание отменено. Повторная отправка доступна вручную.";
  }
  async function clientAction(
    request: ClientRequest,
    accept: (response: PanelResponse) => void,
  ) {
    if (
      disposed ||
      !state.authenticated ||
      state.tab !== "clients" ||
      state.clientsBusy
    )
      return;
    if (state.retryAt > port.now()) {
      state.clientError = "Дождитесь восстановления лимита запросов.";
      return;
    }
    const id = generation,
      operation = ++clientGeneration;
    const controller = new AbortController();
    clientController = controller;
    controllers.add(controller);
    state.clientsBusy = true;
    state.clientError = "";
    const active = () =>
      current(id) && operation === clientGeneration && state.tab === "clients";
    try {
      const response = await port.requestClients(
        request,
        token,
        controller.signal,
      );
      if (!active() || !inspect(response)) return;
      if (response.status === 403) {
        state.clientsAllowed = false;
        state.clients = [];
        clearClientSecret();
        state.clientError =
          "Для управления приложениями войдите с admin-токеном сервера.";
        return;
      }
      if (response.status >= 400) {
        const code = (response.data as { error?: { code?: string } })?.error
          ?.code;
        state.clientError =
          response.status === 409
            ? "Приложение с таким именем уже есть. Выберите другое имя или отзовите прежний ключ."
            : response.status === 404
              ? "Ключ не найден. Обновите список приложений."
              : code === "invalid_client_name"
                ? "Укажите имя из латинских букв, цифр, дефиса и подчёркивания."
                : "Не удалось изменить ключи. Проверьте доступ сервера к хранилищу ключей.";
        return;
      }
      accept(response);
    } catch {
      if (active())
        state.clientError =
          request.method === "GET"
            ? "Не удалось получить список приложений. Повторите обновление."
            : "Ответ не получен. Операция могла выполниться: обновите список перед повтором. Новый ключ можно отозвать и создать заново.";
    } finally {
      controllers.delete(controller);
      if (active()) {
        state.clientsBusy = false;
        clientController = null;
      }
    }
  }
  async function loadClients() {
    await clientAction({ method: "GET" }, (response) => {
      const value = response.data as {
        data?: ServerClient[];
        api_base_url?: string | null;
      };
      if (
        response.status !== 200 ||
        !Array.isArray(value?.data) ||
        value.data.some(
          (item) =>
            !item ||
            !validClientName(item.name) ||
            item.scope !== "recognize" ||
            typeof item.created_at !== "string",
        )
      )
        throw new Error("Invalid client inventory");
      state.clients = value.data.map(({ name, scope, created_at }) => ({
        name,
        scope,
        created_at,
      }));
      state.clientsAllowed = true;
      state.clientsLoaded = true;
      if (!state.clientUrl && value.api_base_url)
        state.clientUrl = apiBaseUrl(value.api_base_url) ?? "";
    });
  }
  async function createClient() {
    if (!state.clientsAllowed || state.clientEnv) return;
    const name = state.clientName.trim();
    if (!validClientName(name)) {
      state.clientError =
        "Имя: 1–64 латинские буквы, цифры, дефис или подчёркивание; первый символ — буква или цифра.";
      return;
    }
    const origin = apiBaseUrl(state.clientUrl);
    if (!origin) {
      state.clientError =
        "Укажите адрес API: http:// или https://, без пути, пароля и параметров.";
      return;
    }
    await clientAction({ method: "POST", name }, (response) => {
      const value = response.data as ServerClient & { token?: string };
      const env = clientEnvironment(origin, value?.token ?? "");
      if (
        response.status !== 201 ||
        value?.name !== name ||
        value.scope !== "recognize" ||
        typeof value.created_at !== "string" ||
        !env
      )
        throw new Error("Invalid created key");
      state.clientEnv = env;
      state.issuedClient = name;
      state.clients = [
        ...state.clients.filter((client) => client.name !== name),
        { name, scope: "recognize", created_at: value.created_at },
      ];
      state.clientName = "";
      state.clientNotice =
        "Ключ создан. Скопируйте .env: после скрытия получить этот ключ повторно нельзя.";
    });
  }
  function askRevoke(name: string) {
    if (
      !state.clientsBusy &&
      state.clients.some((client) => client.name === name)
    )
      state.revokeCandidate = name;
  }
  async function revokeClient() {
    const name = state.revokeCandidate;
    if (!name || !state.clientsAllowed) return;
    await clientAction({ method: "DELETE", name }, (response) => {
      if (response.status !== 204)
        throw new Error("Unexpected revocation response");
      state.clients = state.clients.filter((client) => client.name !== name);
      if (state.issuedClient === name) clearClientSecret();
      state.revokeCandidate = "";
      state.clientNotice = `Ключ приложения ${name} отозван.`;
    });
  }
  async function copyClientEnvironment() {
    if (!state.clientEnv || state.tab !== "clients" || !state.authenticated)
      return;
    const id = generation,
      operation = clientGeneration;
    const env = state.clientEnv;
    try {
      await port.copyText(env);
      if (
        current(id) &&
        operation === clientGeneration &&
        state.clientEnv === env
      )
        state.clientNotice =
          ".env скопирован. Добавьте обе строки в настройки приложения.";
    } catch {
      if (
        current(id) &&
        operation === clientGeneration &&
        state.clientEnv === env
      )
        state.clientNotice =
          "Не удалось скопировать автоматически. Выделите и скопируйте обе строки ниже.";
    }
  }
  const stopTabWatch = watch(
    () => state.tab,
    (tab, previous) => {
      if (previous === "clients") leaveClients();
      if (tab === "clients") void loadClients();
    },
    { flush: "sync" },
  );
  return {
    state,
    login,
    refresh,
    run,
    chooseAudio,
    cancelRequest,
    loadClients,
    createClient,
    askRevoke,
    revokeClient,
    copyClientEnvironment,
    clearClientSecret,
    cancelRevoke() {
      state.revokeCandidate = "";
    },
    logout: clearSession,
    setAutoRefresh(value: boolean) {
      state.autoRefresh = value;
      schedule();
    },
    dispose() {
      stopTabWatch();
      clearSession();
      disposed = true;
    },
  };
}
export type ServerPanel = ReturnType<typeof createServerPanel>;
export const serverPanelKey: InjectionKey<ServerPanel> = Symbol("server-panel");
export function useServerPanel(): ServerPanel {
  const panel = inject(serverPanelKey);
  if (!panel) throw new Error("Server panel was not provided");
  return panel;
}
