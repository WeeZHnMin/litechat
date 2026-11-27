import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { closeContextMenu, showContextMenu } from "../ui/context-menu";

/** 输入栏的工作目录切换：显示当前目录，菜单里可切换或另选。 */
export function initWorkspaceToggle() {
  const button = document.querySelector("#workspace-toggle");
  let current = "";

  /** 按钮只留最后一级目录名，超过 12 字省略中间；完整路径在菜单和悬停里。 */
  function shortName(path) {
    const parts = path.split(/[\\/]/).filter(Boolean);
    const name = parts[parts.length - 1] || path;
    return name.length <= 12 ? name : `${name.slice(0, 6)}…${name.slice(-4)}`;
  }

  /** 菜单里的路径太长时保留首尾，中间省略；悬停可看完整路径。 */
  function middleEllipsis(path, limit = 44) {
    if (path.length <= limit) return path;
    const head = Math.ceil((limit - 1) * 0.55);
    const tail = limit - 1 - head;
    return `${path.slice(0, head)}…${path.slice(-tail)}`;
  }

  function render() {
    const name = current ? shortName(current) : "目录";
    button.textContent = `📁 ${name}`;
    button.title = current ? `工作目录：${current}` : "选择工作目录";
  }

  async function apply(path) {
    const resolved = await invoke("set_workspace", { path });
    current = resolved;
    render();
    window.dispatchEvent(new CustomEvent("litechat:workspace-changed", { detail: resolved }));
    return resolved;
  }

  async function pick() {
    try {
      const chosen = await openDialog({
        directory: true,
        multiple: false,
        defaultPath: current || undefined,
        title: "选择工作目录",
      });
      if (!chosen) return;
      const path = typeof chosen === "string" ? chosen : chosen.path;
      if (path) await apply(path);
    } catch (error) {
      console.error("选择工作目录失败:", error);
    }
  }

  async function openMenu() {
    const rect = button.getBoundingClientRect();
    button.setAttribute("aria-expanded", "true");
    // 打开前同步一次，避免显示的还是旧目录
    await sync();
    let items = [];
    try {
      items = await invoke("get_workspace_history");
    } catch {
      items = [];
    }
    // 历史里可能没有当前目录（用户从没切过），先补进去，
    // 保证菜单里始终能看到完整路径。
    const paths = [current, ...items.filter((path) => path && path !== current)].filter(Boolean);
    const entries = [
      { label: "当前工作目录", disabled: true },
      {
        label: middleEllipsis(current || "（未设置）"),
        checked: true,
        title: current,
        action: () => {},
      },
      { separator: true },
      { label: "选择其他目录…", action: () => void pick() },
      ...(paths.length > 1
        ? [
            { separator: true },
            { label: "最近使用", disabled: true },
            ...paths.slice(1).map((path) => ({
              label: middleEllipsis(path),
              title: path,
              action: () => void apply(path).catch(() => {}),
            })),
          ]
        : []),
    ];
    showContextMenu(rect.left, rect.top - 8, entries, { placement: "above" });
    const reset = () => button.setAttribute("aria-expanded", "false");
    window.addEventListener("pointerdown", reset, { once: true });
  }

  button.addEventListener("click", () => {
    if (button.getAttribute("aria-expanded") === "true") {
      button.setAttribute("aria-expanded", "false");
      closeContextMenu();
      return;
    }
    void openMenu();
  });

  async function sync() {
    try {
      current = await invoke("get_workspace");
    } catch (error) {
      console.error("读取工作目录失败:", error);
    }
    render();
  }

  // 设置页改了目录时同步过来
  window.addEventListener("litechat:workspace-changed", (event) => {
    if (typeof event.detail === "string" && event.detail !== current) {
      current = event.detail;
      render();
    }
  });

  void sync();
  return { sync, get current() { return current; } };
}
