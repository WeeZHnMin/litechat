import { invoke } from "@tauri-apps/api/core";
import { ask, open as openDialog } from "@tauri-apps/plugin-dialog";
import { closeContextMenu, showContextMenu } from "../ui/context-menu";

/** 确认对话框：非 Tauri 环境（浏览器预览）退回原生 confirm。 */
async function confirmDialog(title, message, okLabel) {
  const fallback = () => window.confirm(`${title}\n\n${message}`);
  if (!window.__TAURI_INTERNALS__) return fallback();
  try {
    return await ask(message, { title, kind: "warning", okLabel, cancelLabel: "取消" });
  } catch {
    return fallback();
  }
}

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

  /** 点 × 后先让用户选择：只清历史，还是连磁盘目录一起删。 */
  async function remove(path) {
    const rect = button.getBoundingClientRect();
    showContextMenu(
      rect.left,
      rect.top - 8,
      [
        { label: middleEllipsis(path, 40), disabled: true, title: path },
        { separator: true },
        {
          label: "仅从历史移除",
          action: () => void forgetOnly(path),
        },
        {
          label: "连同磁盘文件一起删除",
          danger: true,
          action: () => void deleteFromDisk(path),
        },
      ],
      { placement: "above" },
    );
  }

  async function forgetOnly(path) {
    const confirmed = await confirmDialog(
      "从历史中移除？",
      `${path}\n\n只会删除这条历史记录，磁盘上的文件夹和其中的文件不受影响，随时可以重新添加。`,
      "移除记录",
    );
    if (!confirmed) return;
    try {
      await invoke("forget_workspace", { path, deleteFiles: false });
    } catch (error) {
      console.error("移除历史目录失败:", error);
    }
    button.setAttribute("aria-expanded", "false");
  }

  /** 连磁盘目录一起删——不可恢复，确认框里明确写清楚。 */
  async function deleteFromDisk(path) {
    const confirmed = await confirmDialog(
      "永久删除这个目录？",
      `${path}\n\n目录及其中的所有文件和子文件夹都会被删除，此操作无法撤销。`,
      "永久删除",
    );
    if (!confirmed) return;
    try {
      await invoke("forget_workspace", { path, deleteFiles: true });
    } catch (error) {
      console.error("删除目录失败:", error);
      window.alert(`删除失败：${error}`);
    }
    button.setAttribute("aria-expanded", "false");
  }

  async function openMenu() {
    const rect = button.getBoundingClientRect();
    button.setAttribute("aria-expanded", "true");
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
              title: `${path}\n点击切换到这个目录`,
              action: () => void apply(path).catch(() => {}),
              onRemove: () => void remove(path),
              removeLabel: `从历史移除 ${path}（不会删除磁盘文件）`,
              removeTitle: "仅从历史记录移除，不删除磁盘上的文件夹",
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

  window.addEventListener("litechat:workspace-changed", (event) => {
    if (typeof event.detail === "string" && event.detail !== current) {
      current = event.detail;
      render();
    }
  });

  void sync();
  return { sync, get current() { return current; } };
}
