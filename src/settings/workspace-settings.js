import { invoke } from "@tauri-apps/api/core";
import { ask, open as openDialog } from "@tauri-apps/plugin-dialog";
import { closeContextMenu, showContextMenu } from "../ui/context-menu";

export function initWorkspaceSettings() {
  const input = document.querySelector("#workspace-path");
  const browse = document.querySelector("#workspace-browse");
  const historyButton = document.querySelector("#workspace-history");
  const hint = document.querySelector("#workspace-hint");
  const view = document.querySelector("#settings-view");
  let current = "";

  function setHint(text, isError) {
    hint.textContent = text;
    hint.classList.toggle("is-error", Boolean(isError));
  }

  function apply(path) {
    current = path;
    input.value = path;
    input.title = path;
    setHint("所有相对路径都基于此目录", false);
  }

  async function load() {
    try {
      apply(await invoke("get_workspace"));
    } catch (error) {
      setHint(String(error), true);
    }
  }

  async function commit() {
    save.disabled = true;
    try {
      const resolved = await invoke("set_workspace", { path: input.value.trim() });
      apply(resolved);
      window.dispatchEvent(new CustomEvent("litechat:workspace-changed", { detail: resolved }));
      setHint("已保存", false);
      window.setTimeout(() => setHint("所有相对路径都基于此目录", false), 1800);
    } catch (error) {
      setHint(String(error), true);
    } finally {
      save.disabled = false;
    }
  }

  async function pickDirectory() {
    if (!window.__TAURI_INTERNALS__) {
      setHint("当前环境不支持选择目录，请手动填写路径", true);
      return;
    }
    try {
      const chosen = await openDialog({
        directory: true,
        multiple: false,
        defaultPath: current || undefined,
        title: "选择工作目录",
      });
      if (!chosen) return;
      const path = typeof chosen === "string" ? chosen : chosen.path;
      if (path) {
        input.value = path;
        await commit();
      }
    } catch (error) {
      setHint(String(error), true);
    }
  }

  async function openHistory() {
    if (historyButton.getAttribute("aria-expanded") === "true") {
      historyButton.setAttribute("aria-expanded", "false");
      closeContextMenu();
      return;
    }
    historyButton.setAttribute("aria-expanded", "true");
    let items = [];
    try {
      items = await invoke("get_workspace_history");
    } catch {
      items = [];
    }
    const rect = historyButton.getBoundingClientRect();
    const entries = items.length
      ? items.map((path) => ({
          label: path,
          title: path,
          checked: path === current,
          action: () => {
            input.value = path;
            void commit();
          },
          onRemove:
            path === current
              ? null
              : () => void forget(path),
          removeLabel: `从历史移除 ${path}（不会删除磁盘文件）`,
          removeTitle: "仅从历史记录移除，不删除磁盘上的文件夹",
        }))
      : [{ label: "还没有切换过目录", disabled: true }];
    showContextMenu(rect.left, rect.bottom + 4, entries);
    const reset = () => historyButton.setAttribute("aria-expanded", "false");
    window.addEventListener("pointerdown", reset, { once: true });
  }

  /** 点 × 后让用户选：只清历史，还是连磁盘一起删。 */
  async function forget(path) {
    const rect = historyButton.getBoundingClientRect();
    showContextMenu(rect.left, rect.bottom + 4, [
      { label: path, disabled: true, title: path },
      { separator: true },
      { label: "仅从历史移除", action: () => void forgetOnly(path) },
      {
        label: "连同磁盘文件一起删除",
        danger: true,
        action: () => void deleteFromDisk(path),
      },
    ]);
  }

  async function forgetOnly(path) {
    const message = `${path}\n\n只会删除这条历史记录，磁盘上的文件夹和其中的文件不受影响，随时可以重新添加。`;
    if (!(await confirm(message, "从历史中移除？", "移除记录"))) return;
    try {
      await invoke("forget_workspace", { path, deleteFiles: false });
      historyButton.setAttribute("aria-expanded", "false");
      closeContextMenu();
      setHint("已从历史移除（磁盘文件未改动）", false);
    } catch (error) {
      setHint(String(error), true);
    }
  }

  async function deleteFromDisk(path) {
    const message = `${path}\n\n目录及其中的所有文件和子文件夹都会被删除，此操作无法撤销。`;
    if (!(await confirm(message, "永久删除这个目录？", "永久删除"))) return;
    try {
      await invoke("forget_workspace", { path, deleteFiles: true });
      historyButton.setAttribute("aria-expanded", "false");
      closeContextMenu();
      setHint("已删除目录", false);
    } catch (error) {
      setHint(String(error), true);
    }
  }

  /** 统一确认：Tauri 下用系统对话框，浏览器预览退回 confirm。 */
  async function confirm(message, title, okLabel) {
    if (!window.__TAURI_INTERNALS__) return window.confirm(`${title}

${message}`);
    try {
      return await ask(message, { title, kind: "warning", okLabel, cancelLabel: "取消" });
    } catch {
      return window.confirm(`${title}

${message}`);
    }
  }


  browse.addEventListener("click", () => void pickDirectory());
  historyButton.addEventListener("click", () => void openHistory());
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && !event.isComposing) {
      event.preventDefault();
      void commit();
    }
  });
  input.addEventListener("blur", () => {
    const value = input.value.trim();
    if (value && value !== current) void commit();
  });

  new MutationObserver(() => {
    if (!view.hidden && input.value.trim() === "") void load();
  }).observe(view, { attributes: true, attributeFilter: ["hidden"] });

  void load();

  return { reload: load, get current() { return current; } };
}
