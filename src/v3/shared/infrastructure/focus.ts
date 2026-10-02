export function focusField(id: string): void {
  const target = document.getElementById(id);
  if (!target) return;
  if (target instanceof HTMLDetailsElement) target.open = true;
  target.scrollIntoView({ behavior: "instant", block: "center" });
  const focusable = target.matches(
    "input, textarea, select, button, [tabindex]",
  )
    ? target
    : target.querySelector<HTMLElement>(
        "input, textarea, select, button, summary, [tabindex]",
      );
  (focusable as HTMLElement | null)?.focus({ preventScroll: true });
  target.classList.add("field-highlight");
  setTimeout(() => target.classList.remove("field-highlight"), 1800);
}
