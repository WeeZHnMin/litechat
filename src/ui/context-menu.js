let menu = null;

export function closeContextMenu() {
  if (!menu) return;
  menu.remove();
  menu = null;
}

export function showContextMenu(x, y, items, { placement = "auto", searchable = false } = {}) {
  closeContextMenu();
  const entries = items.filter(Boolean);
  if (!entries.length) return;
  const element = document.createElement("div");
  element.className = "context-menu";
  element.setAttribute("role", "menu");

  const maxHeight = Math.min(360, Math.round(window.innerHeight * 0.5));
  element.style.setProperty("--menu-max-height", `${Math.max(160, maxHeight)}px`);

  const renderItems = (list) => {
    element.replaceChildren();
    let search = null;
    if (searchable) {
      search = document.createElement("input");
      search.type = "search";
      search.className = "context-menu-search";
      search.placeholder = "搜索…";
      search.spellcheck = false;
      element.append(search);
      search.addEventListener("input", () => {
        const keyword = search.value.trim().toLowerCase();
        const matched = list.filter((item) => !keyword || item.label.toLowerCase().includes(keyword));
        renderButtons(matched, search);
        search.focus();
      });
    }
    renderButtons(list, search);
    return search;
  };

  const renderButtons = (list, searchToKeep) => {
    for (const node of [...element.children]) {
      if (node !== searchToKeep) node.remove();
    }
    if (!list.length) {
      const empty = document.createElement("div");
      empty.className = "context-menu-empty";
      empty.textContent = "没有匹配项";
      element.append(empty);
      return;
    }
    for (const item of list) {
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
      if (item.checked) {
        button.classList.add("is-checked");
        button.setAttribute("aria-checked", "true");
      }
      if (item.danger) button.classList.add("is-danger");
      if (item.disabled) button.disabled = true;
      button.title = item.title ?? item.label;
      button.addEventListener("click", () => {
        closeContextMenu();
        void item.action?.();
      });

      // 带删除的条目：主体仍是按钮，右侧挂一个独立的移除控件，
      // 点删除不会触发这一条的切换动作。
      if (item.onRemove) {
        const row = document.createElement("div");
        row.className = "context-menu-row";
        const remove = document.createElement("button");
        remove.type = "button";
        remove.className = "context-menu-remove";
        remove.setAttribute("aria-label", item.removeLabel ?? `移除 ${item.label}`);
        remove.title = item.removeTitle ?? "从历史中移除";
        remove.textContent = "×";
        remove.addEventListener("click", (event) => {
          event.stopPropagation();
          void item.onRemove();
        });
        row.append(button, remove);
        element.append(row);
        continue;
      }
      element.append(button);
    }
  };

  const searchInput = renderItems(entries);
  position(element, x, y, placement);
  menu = element;
  searchInput?.focus();
  element.addEventListener("contextmenu", (event) => event.preventDefault());
  window.addEventListener("blur", closeContextMenu, { once: true });
  window.addEventListener("resize", closeContextMenu, { once: true });
  return element;
}

/** 按视口余量决定菜单开在点击点下方还是上方。 */
function position(element, x, y, placement) {
  document.body.append(element);
  const rect = element.getBoundingClientRect();
  const margin = 8;
  const left = Math.min(x, window.innerWidth - rect.width - margin);

  let top;
  if (placement === "above") {
    top = y - rect.height;
    // 上方放不下就翻到下方，再不行就贴边
    if (top < margin) {
      const spaceBelow = window.innerHeight - y - margin;
      top = rect.height <= spaceBelow ? y : margin;
    }
  } else {
    const spaceBelow = window.innerHeight - y - margin;
    const spaceAbove = y - margin;
    top = rect.height > spaceBelow && spaceAbove > spaceBelow ? y - rect.height : y;
  }

  element.style.left = `${Math.max(margin, left)}px`;
  element.style.top = `${Math.max(margin, Math.min(top, window.innerHeight - rect.height - margin))}px`;
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
