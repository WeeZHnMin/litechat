let menu = null;

export function closeContextMenu() {
  if (!menu) return;
  menu.remove();
  menu = null;
}

function place(element, x, y) {
  document.body.append(element);
  const rect = element.getBoundingClientRect();
  const margin = 8;
  const left = Math.min(x, window.innerWidth - rect.width - margin);
  const top = Math.min(y, window.innerHeight - rect.height - margin);
  element.style.left = `${Math.max(margin, left)}px`;
  element.style.top = `${Math.max(margin, top)}px`;
}

export function showContextMenu(x, y, items, { placement = "auto" } = {}) {
  closeContextMenu();
  const entries = items.filter(Boolean);
  if (!entries.length) return;
  const element = document.createElement("div");
  element.className = "context-menu";
  element.setAttribute("role", "menu");
  for (const item of entries) {
    if (item.separator) {
      const line = document.createElement("div");
      line.className = "context-menu-separator";
      element.append(line);
      continue;
    }
    const button = document.createElement("button");
    button.type = "button";
    button.className = "context-menu-item";
    button.setAttribute("role", "menuitem");
    button.textContent = item.label;
    if (item.danger) button.classList.add("is-danger");
    if (item.disabled) button.disabled = true;
    button.addEventListener("click", () => {
      closeContextMenu();
      void item.action?.();
    });
    element.append(button);
  }
  if (placement === "above") {
    document.body.append(element);
    const rect = element.getBoundingClientRect();
    const margin = 8;
    element.style.left = `${Math.max(margin, x)}px`;
    element.style.top = `${Math.max(margin, y - rect.height)}px`;
  } else {
    place(element, x, y);
  }
  menu = element;
  element.addEventListener("contextmenu", (event) => event.preventDefault());
  window.addEventListener("blur", closeContextMenu, { once: true });
  window.addEventListener("resize", closeContextMenu, { once: true });
  return element;
}

export function installGlobalContextMenuGuard(shouldAllow) {
  document.addEventListener("contextmenu", (event) => {
    if (shouldAllow?.(event.target)) return;
    event.preventDefault();
  });
  document.addEventListener("pointerdown", (event) => {
    if (menu && !menu.contains(event.target)) closeContextMenu();
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && menu) {
      event.preventDefault();
      event.stopPropagation();
      closeContextMenu();
    }
  }, true);
}
