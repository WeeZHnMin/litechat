import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
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

  /** 用系统原生目录选择器挑一个目录，填进输入框。 */
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

  /** 历史工作目录：点一下直接切换过去。 */
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
          checked: path === current,
          action: () => {
            input.value = path;
            void commit();
          },
        }))
      : [{ label: "还没有切换过目录", disabled: true }];
    showContextMenu(rect.left, rect.bottom + 4, entries);
    const reset = () => historyButton.setAttribute("aria-expanded", "false");
    window.addEventListener("pointerdown", reset, { once: true });
  }

  browse.addEventListener("click", () => void pickDirectory());
  historyButton.addEventListener("click", () => void openHistory());
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && !event.isComposing) {
      event.preventDefault();
      void commit();
    }
  });
  // 手改路径后离开输入框就保存，不用再找按钮
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
