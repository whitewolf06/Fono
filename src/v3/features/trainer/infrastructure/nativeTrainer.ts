import type { TrainerPort } from "../../../shared/domain/contracts";
import type { TrainerReport } from "../domain/report";
import { observeTrainerDay } from "./dayChange";

type Invoke = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;
export function createNativeTrainer(
  call: Invoke,
  readHistory: () => Promise<void>,
  newId: () => string = () => crypto.randomUUID(),
): TrainerPort {
  let active: string | null = null;
  return {
    onDayChange: observeTrainerDay,
    getReport: (days, refresh = false) =>
      call<TrainerReport>("get_trainer_report", { days, refresh }),
    async analyzePeriod(input) {
      if (active) throw new Error("Дождитесь завершения текущего разбора.");
      const requestId = newId();
      active = requestId;
      try {
        return await call<TrainerReport>("analyze_trainer_period", {
          ...input,
          requestId,
        });
      } finally {
        if (active === requestId) active = null;
      }
    },
    async cancelPeriod() {
      if (active) await call("cancel_trainer_period", { requestId: active });
    },
    async clear() {
      if (active) await call("cancel_trainer_period", { requestId: active });
      await call("clear_speech_analytics");
      await readHistory();
    },
    async recommend(id) {
      const result = await call<{
        summary: string;
        recommendations: { title: string; exercise: string }[];
      }>("recommend_speech", { id });
      return [
        result.summary,
        ...result.recommendations.map(
          (item) => `${item.title}: ${item.exercise}`,
        ),
      ].join("\n\n");
    },
  };
}
