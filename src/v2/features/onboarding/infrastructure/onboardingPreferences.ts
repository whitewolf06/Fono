const completionStorageKey = "fono-ui-v2-onboarding-complete";

export function shouldShowOnboarding(): boolean {
  try {
    return window.localStorage.getItem(completionStorageKey) !== "true";
  } catch {
    return true;
  }
}

export function completeOnboarding(): void {
  try {
    window.localStorage.setItem(completionStorageKey, "true");
  } catch {
    // The introduction remains dismissible when persistent storage is unavailable.
  }
}
