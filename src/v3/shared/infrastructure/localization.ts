import { reactive, watch, type App } from "vue";
import { wlLocaleRu } from "@whitelife-core/ui-kit";
import {
  bindMessageLocale,
  resolveUiLocale,
  type UiLanguage,
  type UiLocale,
} from "../domain/locale";
import { message } from "../domain/messages";
const state = reactive({
  language: "auto" as UiLanguage,
  systemLanguage: "en",
  locale: "ru" as UiLocale,
});
const whiteUiEnglish = {
  ...wlLocaleRu,
  firstDayOfWeek: 1,
  dayNames: [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
  ],
  dayNamesShort: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
  dayNamesMin: ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"],
  monthNames: [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
  ],
  monthNamesShort: [
    "Jan",
    "Feb",
    "Mar",
    "Apr",
    "May",
    "Jun",
    "Jul",
    "Aug",
    "Sep",
    "Oct",
    "Nov",
    "Dec",
  ],
  today: "Today",
  clear: "Clear",
  accept: "OK",
  reject: "Cancel",
  chooseDate: "Choose a date",
  prevMonth: "Previous month",
  nextMonth: "Next month",
};
export const whiteUiLocale = reactive({ ...wlLocaleRu });
export function browserSystemLanguage(): string {
  return typeof navigator === "undefined" ? "en" : navigator.language;
}
export function initializeLocalization(
  language: UiLanguage,
  systemLanguage: string,
) {
  state.systemLanguage = systemLanguage;
  bindMessageLocale(() => state.locale);
  setUiLanguage(language);
}
export function setUiLanguage(language: UiLanguage) {
  state.language = language;
  state.locale = resolveUiLocale(language, state.systemLanguage);
  Object.assign(
    whiteUiLocale,
    state.locale === "ru" ? wlLocaleRu : whiteUiEnglish,
  );
  if (typeof document !== "undefined")
    document.documentElement.lang = state.locale;
}
export function installMessages(app: App) {
  app.config.globalProperties.$t = message;
}
export function followUiLanguage(read: () => UiLanguage) {
  return watch(read, setUiLanguage);
}
declare module "vue" {
  interface ComponentCustomProperties {
    $t: typeof message;
  }
}
