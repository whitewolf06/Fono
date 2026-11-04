import { reactive, type InjectionKey, inject } from "vue";
import {
  operationsFrom,
  type ApiOperation,
  type PanelAudio,
  type PanelPort,
  type PanelRequest,
  type PanelResponse,
  type PanelTab,
  type ServerCapabilities,
  type ServerHealth,
  type ServerModel,
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
  });
  let token = "",
    generation = 0,
    requestGeneration = 0,
    disposed = false;
  let stopTimer = () => {};
  const controllers = new Set<AbortController>();
  let requestController: AbortController | null = null;

  function clearSession() {
    generation++;
    requestGeneration++;
    token = "";
    stopTimer();
    for (const controller of controllers) controller.abort();
    controllers.clear();
    requestController = null;
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
  return {
    state,
    login,
    refresh,
    run,
    chooseAudio,
    cancelRequest,
    logout: clearSession,
    setAutoRefresh(value: boolean) {
      state.autoRefresh = value;
      schedule();
    },
    dispose() {
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
