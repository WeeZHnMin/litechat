import { invoke } from "@tauri-apps/api/core";

export function initWindowControls({ onNewChat }) {
  const pinButton = document.querySelector("#pin");
  document.querySelector("#minimize").addEventListener("click", async () => {
    try {
      await invoke("minimize_window");
    } catch (error) {
      console.error("Unable to minimize the window:", error);
    }
  });
  pinButton.addEventListener("click", async () => {
    try {
      const isPinned = await invoke("toggle_always_on_top");
      pinButton.setAttribute("aria-pressed", String(isPinned));
      pinButton.title = isPinned ? "取消钉在桌面" : "钉在桌面";
    } catch (error) {
      console.error("Unable to change always-on-top state:", error);
    }
  });
  document.querySelector("#new-chat").addEventListener("click", onNewChat);
}
