import { ref } from "vue";
import { useWlToast } from "@whitelife-core/ui-kit";
export function useFeedback() {
  const toast = useWlToast();
  const busy = ref(false);
  const error = ref("");
  async function run(
    action: () => unknown | Promise<unknown>,
    success?: string,
  ) {
    busy.value = true;
    error.value = "";
    try {
      await action();
      if (success) toast.ok(success);
      return true;
    } catch (e) {
      error.value =
        e instanceof Error ? e.message : "Не удалось выполнить действие.";
      toast.err(error.value);
      return false;
    } finally {
      busy.value = false;
    }
  }
  return { run, busy, error };
}
