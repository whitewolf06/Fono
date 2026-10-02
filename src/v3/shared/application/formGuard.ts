import { onScopeDispose } from "vue";
import { useRouter } from "vue-router";
import { useInteraction } from "./interaction";
import { protectUnload } from "../infrastructure/browser";
export function useFormGuard(
  isDirty: () => boolean,
  onClose: () => void,
  isBusy = () => false,
) {
  const router = useRouter();
  const ui = useInteraction();
  async function canDiscard() {
    if (isBusy()) return false;
    return (
      !isDirty() ||
      ui.confirm({
        title: "Закрыть без сохранения?",
        text: "В форме есть несохранённые изменения. При выходе они будут отменены.",
        accept: "Отменить изменения",
        cancel: "Остаться",
      })
    );
  }
  const release = router.beforeEach(async () => {
    if (!(await canDiscard())) return false;
    onClose();
  });
  onScopeDispose(release);
  onScopeDispose(protectUnload(isDirty));
  return canDiscard;
}
