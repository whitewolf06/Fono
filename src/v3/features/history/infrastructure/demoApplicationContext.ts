import type { Dictation } from "../../../shared/domain/contracts";
/** Synthetic fixture only; no foreground probes, disk or browser persistence. */
export function addDemoApplicationContext(entries: Dictation[]) {
  for (const [i, e] of entries.slice(0, 3).entries()) {
    const name = ["Code", "Notepad", "chrome"][i];
    const app = { id: `exe:${name.toLowerCase()}.exe`, name };
    e.applicationContext = {
      before: app,
      after: i === 1 ? null : app,
      outcome: i === 1 ? "focus_changed" : "sent",
      method: "clipboard",
      attempted: true,
      demo: true,
    };
  }
}
