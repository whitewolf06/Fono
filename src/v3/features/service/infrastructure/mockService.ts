import type {
  ServicePort,
  WorkspaceState,
} from "../../../shared/domain/contracts";
export function createServicePort(state: WorkspaceState): ServicePort {
  return {
    cancel(id) {
      const job = state.jobs.find((j) => j.id === id);
      if (job && ["queued", "running"].includes(job.state))
        job.state = "cancelled";
    },
    enqueue() {
      if (!state.preferences.serviceEnabled)
        throw new Error("Сначала включите API-сервис.");
      if (
        state.jobs.filter((j) => ["queued", "running"].includes(j.state))
          .length >= 4
      )
        throw new Error(
          "Очередь заполнена. Дождитесь завершения или отмените задачу.",
        );
      state.jobs.unshift({
        id: "job-" + Date.now(),
        name: "demo-recording.wav",
        state: "queued",
        seconds: 18,
      });
    },
    retry(id) {
      const job = state.jobs.find((j) => j.id === id);
      if (job && state.preferences.serviceEnabled) {
        job.state = "queued";
        job.error = undefined;
      }
    },
  };
}
