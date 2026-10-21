import { message as uiMessage } from "../domain/messages";
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
        get title() {
          return uiMessage("ui.0e12324ee6");
        },
        get text() {
          return uiMessage("ui.d770d39152");
        },
        get accept() {
          return uiMessage("ui.9471b91e84");
        },
        get cancel() {
          return uiMessage("ui.3a5acd0245");
        },
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
