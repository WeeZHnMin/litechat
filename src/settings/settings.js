import { invoke } from "@tauri-apps/api/core";
import { ShortcutGroup } from "../ui/shortcut-recorder";
import { PanelView } from "../app/views";

export class SettingsView extends PanelView {
  #group;
  #errorLabel;
  #suspended = false;

  constructor(options) {
    super(options);
    this.#errorLabel = document.querySelector("#shortcut-error");
    const buttons = [...document.querySelectorAll(".shortcut-key[data-action]")];
    const elements = Object.fromEntries(buttons.map((item) => [item.dataset.action, item]));
    this.#group = new ShortcutGroup({
      elements,
      config: { toggle: "Alt+Shift+L", newChat: "Alt+Shift+N", history: "Alt+Shift+H" },
      onApply: (next) => this.#applyShortcuts(next),
      onRecordingChange: (recording) => this.#setRecording(recording),
    });
    this.toggleButton.addEventListener("click", () => this.manager.toggle(this));
    document.querySelector("#settings-back").addEventListener("click", () => this.manager.close(this));
    void invoke("get_shortcuts")
      .then((saved) => this.#group.apply(saved))
      .catch((error) => console.error("Unable to load shortcuts:", error));
  }

  activate() {
    this.toggleButton.title = "返回聊天";
    super.activate();
  }

  deactivate() {
    this.toggleButton.title = "设置";
    this.#setRecording(false);
    super.deactivate();
  }

  handleKeyDown(event) {
    if (!this.#group.isRecording) return false;
    event.stopPropagation();
    this.#group.handleEvent(event);
    return true;
  }

  async #applyShortcuts(next) {
    this.#errorLabel.hidden = true;
    try {
      return await invoke("update_shortcut", next);
    } catch (error) {
      this.#errorLabel.textContent = String(error);
      this.#errorLabel.hidden = false;
      throw error;
    }
  }

  #setRecording(recording) {
    if (this.#suspended === recording) return;
    this.#suspended = recording;
    void invoke("set_shortcuts_suspended", { suspended: recording }).catch((error) =>
      console.error("Unable to toggle global shortcuts:", error),
    );
  }
}

export function initSettings(manager) {
  return new SettingsView({
    name: "settings",
    manager,
    element: document.querySelector("#settings-view"),
    toggleButton: document.querySelector("#settings-button"),
    initialFocus: document.querySelector("#settings-back"),
    // Esc / 返回按钮回到聊天后直接能继续打字
    returnFocus: document.querySelector("#message-input"),
  });
}
