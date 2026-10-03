import type { WorkspaceState } from "../../../shared/domain/contracts";
import type { WakePort, WakeSetupState } from "../../../shared/domain/wake";

const wait = (ms: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));
export function createMockWake(
  state: WorkspaceState,
): WakePort & { resetDemo(): void } {
  let language = state.preferences.wakeLanguage;
  let generation = 0;
  const fresh = (): WakeSetupState => ({
    modelReady: true,
    profileReady: false,
    verified: false,
    active: false,
    required: 5,
    accepted: 0,
    rejected: 0,
    phrase: state.preferences.wakePhrase,
    validation: {
      active: false,
      completed: false,
      failed: false,
      positivePassed: 0,
      positiveRequired: 3,
      silencePassed: false,
      otherPhrasePassed: false,
    },
  });
  function resetDemo() {
    generation++;
    language = state.preferences.wakeLanguage;
    state.wakeSetup = {
      ...fresh(),
      profileReady: true,
      verified: true,
      accepted: 5,
      validation: {
        active: false,
        completed: true,
        failed: false,
        positivePassed: 3,
        positiveRequired: 3,
        silencePassed: true,
        otherPhrasePassed: true,
      },
    };
  }
  resetDemo();
  state.wakeCapabilities = {
    backend: "mock",
    customPhrase: true,
    languages: [
      { value: "ru", label: "Русский" },
      { value: "en", label: "Английский" },
    ],
  };
  function load() {
    if (
      state.wakeSetup?.phrase !== state.preferences.wakePhrase ||
      language !== state.preferences.wakeLanguage
    )
      state.wakeSetup = fresh();
    language = state.preferences.wakeLanguage;
    return Promise.resolve();
  }
  return {
    resetDemo,
    load,
    async download() {
      await wait(300);
      state.wakeSetup!.modelReady = true;
    },
    async test() {
      await wait(400);
      return "Демонстрация: фраза обнаружена. Микрофон и модель не используются.";
    },
    async begin() {
      generation++;
      state.wakeSetup = { ...fresh(), active: true };
      language = state.preferences.wakeLanguage;
    },
    async record() {
      const s = state.wakeSetup!;
      if (!s.active) throw new Error("Сначала начните настройку фразы.");
      const ticket = generation;
      await wait(300);
      if (ticket !== generation) return;
      s.accepted++;
      s.latest = { accepted: true, detected: true };
      if (s.accepted >= s.required) {
        s.active = false;
        s.profileReady = true;
      }
    },
    async beginValidation() {
      const s = state.wakeSetup!;
      if (!s.profileReady) throw new Error("Сначала запишите пять образцов.");
      s.validation = {
        active: true,
        completed: false,
        failed: false,
        positivePassed: 0,
        positiveRequired: 3,
        silencePassed: false,
        otherPhrasePassed: false,
      };
    },
    async validate(kind) {
      const s = state.wakeSetup!;
      if (!s.validation.active)
        throw new Error("Сначала начните контрольную проверку.");
      const ticket = generation;
      await wait(300);
      if (ticket !== generation) return;
      if (kind === "positive")
        s.validation.positivePassed = Math.min(
          3,
          s.validation.positivePassed + 1,
        );
      if (kind === "silence") s.validation.silencePassed = true;
      if (kind === "other_phrase") s.validation.otherPhrasePassed = true;
      if (
        s.validation.positivePassed === 3 &&
        s.validation.silencePassed &&
        s.validation.otherPhrasePassed
      ) {
        s.validation.active = false;
        s.validation.completed = true;
        s.verified = true;
      }
    },
    async cancel() {
      generation++;
      state.wakeSetup!.active = false;
      state.wakeSetup!.validation.active = false;
    },
  };
}
