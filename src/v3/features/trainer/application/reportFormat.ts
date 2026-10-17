/** Trainer snapshots use UTC calendar days independently of the local timezone. */
export const reportDate = (value: string) =>
  new Date(value).toLocaleDateString("ru-RU", {
    day: "numeric",
    month: "short",
    timeZone: "UTC",
  });
export const reportTime = (value: string) =>
  new Date(value).toLocaleString("ru-RU", {
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
    timeZone: "UTC",
  });
