import { invoke } from "@tauri-apps/api/core";
import { closeContextMenu, showContextMenu } from "../ui/context-menu";

const MODES = [
  { value: "ask", label: "每次询问", hint: "敏感操作前弹窗确认" },
  { value: "session", label: "会话内记住", hint: "本会话允许过的工具不再问" },
  { value: "always", label: "全部允许", hint: "直接执行，不再确认" },
];

function labelOf(mode) {
  return MODES.find((item) => item.value === mode)?.label ?? "每次询问";
}

export function initPermissionToggle() {
  const button = document.querySelector("#permission-toggle");
  let mode = "ask";

  function render() {
    button.textContent = labelOf(mode);
    button.setAttribute("aria-pressed", String(mode !== "ask"));
    button.classList.toggle("is-open-mode", mode === "always");
  }

  function openMenu() {
    const rect = button.getBoundingClientRect();
    button.setAttribute("aria-expanded", "true");
    showContextMenu(
      rect.left,
      rect.top - 8,
      MODES.map((item) => ({
        label: `${item.label} · ${item.hint}`,
        action: () => void apply(item.value),
      })),
      { placement: "above" },
    );
    const reset = () => button.setAttribute("aria-expanded", "false");
    window.addEventListener("pointerdown", reset, { once: true });
  }

  async function apply(value) {
    if (value === mode) return;
    try {
      const state = await invoke("set_approval_mode", { mode: value });
      mode = state.mode;
      render();
    } catch (error) {
      console.error("Unable to change approval mode:", error);
    }
  }

  async function sync() {
    try {
      const state = await invoke("get_permission_state");
      mode = state.mode;
      render();
    } catch {
      // 浏览器预览环境没有 Tauri
    }
  }

  button.addEventListener("click", () => {
    if (button.getAttribute("aria-expanded") === "true") {
      button.setAttribute("aria-expanded", "false");
      closeContextMenu();
      return;
    }
    openMenu();
  });

  void sync();
  return { sync, get mode() { return mode; } };
}
