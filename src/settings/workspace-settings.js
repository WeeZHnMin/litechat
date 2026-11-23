import { invoke } from "@tauri-apps/api/core";

export function initWorkspaceSettings() {
  const input = document.querySelector("#workspace-path");
  const save = document.querySelector("#workspace-save");
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
      setHint("已保存", false);
      window.setTimeout(() => setHint("所有相对路径都基于此目录", false), 1800);
    } catch (error) {
      setHint(String(error), true);
    } finally {
      save.disabled = false;
    }
  }

  save.addEventListener("click", commit);
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && !event.isComposing) {
      event.preventDefault();
      void commit();
    }
  });

  new MutationObserver(() => {
    if (!view.hidden && input.value.trim() === "") void load();
  }).observe(view, { attributes: true, attributeFilter: ["hidden"] });

  void load();

  return { reload: load, get current() { return current; } };
}
