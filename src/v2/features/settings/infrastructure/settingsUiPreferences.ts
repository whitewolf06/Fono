import type { SettingsSection } from "../application/useSettingsDraft";

const storageKey = "fono-ui-v2-settings-collapsed-sections";

export function loadCollapsedSettingsSections(): SettingsSection[] {
  try {
    const rawValue = window.localStorage.getItem(storageKey);

    if (!rawValue) return [];

    const parsedValue: unknown = JSON.parse(rawValue);

    if (!Array.isArray(parsedValue)) return [];

    return parsedValue.filter(isSettingsSection);
  } catch {
    return [];
  }
}

export function saveCollapsedSettingsSections(sections: SettingsSection[]) {
  window.localStorage.setItem(storageKey, JSON.stringify(sections));
}

function isSettingsSection(value: unknown): value is SettingsSection {
  return (
    value === "general" ||
    value === "audio" ||
    value === "activation" ||
    value === "processing" ||
    value === "overlay" ||
    value === "advanced"
  );
}
