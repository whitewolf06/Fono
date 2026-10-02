export function AppIcon() {
  return (
    <img
      className="v2-app-icon"
      src={import.meta.env.BASE_URL + "fono-icon.png"}
      width={34}
      height={34}
      alt=""
      aria-hidden="true"
      draggable={false}
    />
  );
}
