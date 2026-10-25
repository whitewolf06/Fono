export function focusField(id: string, fallback?: string): void {
  const target =
    document.getElementById(id) ||
    (fallback ? document.getElementById(fallback) : null);
  if (!target) return;
  const ancestors: HTMLDetailsElement[] = [];
  for (
    let element: HTMLElement | null = target;
    element;
    element = element.parentElement
  ) {
    if (element instanceof HTMLDetailsElement) ancestors.push(element);
  }
  for (const details of ancestors.reverse()) details.open = true;
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
