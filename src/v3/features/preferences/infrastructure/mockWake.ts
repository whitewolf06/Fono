import { message as uiMessage } from "../../../shared/domain/messages";
import type { WorkspaceState } from "../../../shared/domain/contracts";
import type { WakePort, WakeSetupState } from "../../../shared/domain/wake";
import { ensureWakeAvailable } from "../../../shared/domain/wakeAvailability";
import { createFrozenWake } from "../../../shared/infrastructure/frozenWake";

const wait = (ms: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));
export function createMockWake(
  state: WorkspaceState,
): WakePort & { resetDemo(): void } {
  if (!(typeof __FONO_WAKE_WORD__ !== "undefined" && __FONO_WAKE_WORD__)) {
    state.wakeSetup = {
      modelReady: false,
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
    };
    return { ...createFrozenWake(), resetDemo() {} };
  }
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
      {
        value: "ru",
        get label() {
          return uiMessage("ui.6da140fdd4");
        },
      },
      {
        value: "en",
        get label() {
          return uiMessage("ui.e7c1ef1d8e");
        },
      },
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
      ensureWakeAvailable();
      await wait(300);
      state.wakeSetup!.modelReady = true;
    },
    async test() {
      ensureWakeAvailable();
      await wait(400);
      return uiMessage("ui.e6180eea2e");
    },
    async begin() {
      ensureWakeAvailable();
      generation++;
      state.wakeSetup = { ...fresh(), active: true };
      language = state.preferences.wakeLanguage;
    },
    async record() {
      ensureWakeAvailable();
      const s = state.wakeSetup!;
      if (!s.active) throw new Error(uiMessage("ui.ef6cfd311a"));
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
      ensureWakeAvailable();
      const s = state.wakeSetup!;
      if (!s.profileReady) throw new Error(uiMessage("ui.8c1234f1f5"));
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
      ensureWakeAvailable();
      const s = state.wakeSetup!;
      if (!s.validation.active) throw new Error(uiMessage("ui.f3bf3087d0"));
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
