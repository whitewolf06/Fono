const toggle = document.querySelector("[data-menu-toggle]");
const navigation = document.querySelector("[data-site-nav]");

if (toggle && navigation) {
  const mobile = window.matchMedia("(max-width: 719px)");
  const closeMenu = () => {
    toggle.setAttribute("aria-expanded", "false");
    navigation.dataset.open = "false";
  };
  toggle.hidden = false;
  document.documentElement.classList.add("has-navigation-js");
  closeMenu();
  toggle.addEventListener("click", () => {
    const open = toggle.getAttribute("aria-expanded") !== "true";
    toggle.setAttribute("aria-expanded", String(open));
    navigation.dataset.open = String(open);
  });
  document.addEventListener("keydown", (event) => {
    if (
      event.key === "Escape" &&
      toggle.getAttribute("aria-expanded") === "true"
    ) {
      closeMenu();
      toggle.focus();
    }
  });
  navigation.addEventListener("click", (event) => {
    if (event.target.closest("a")) closeMenu();
  });
  mobile.addEventListener("change", closeMenu);
}
