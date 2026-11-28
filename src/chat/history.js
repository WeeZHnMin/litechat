import { invoke } from "@tauri-apps/api/core";
import { createScrollMemory } from "./scroll-memory";
import { PanelView } from "../app/views";

export class HistoryView extends PanelView {
  #appendMessage;
  #clearConversation;
  #list;
  #dateFormatter = new Intl.DateTimeFormat("zh-CN", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
  #conversations = [];
  #selectedIndex = 0;
  #currentId = null;
  #generation = 0;
  #queue = Promise.resolve();
  #scroll;

  constructor(options) {
    super(options);
    this.#appendMessage = options.appendMessage;
    this.#clearConversation = options.clearConversation;
    this.onEnter = options.onEnter;
    this.isRunning = options.isRunning ?? null;
    this.#list = document.querySelector("#history-list");
    this.#scroll = createScrollMemory(options.conversationElement);
    this.toggleButton.addEventListener("click", () => this.manager.toggle(this));
    document.querySelector("#history-back").addEventListener("click", () => this.manager.close(this));
    this.element.addEventListener("focusout", () => this.#scroll.capture());
  }

  get currentId() {
    return this.#currentId;
  }

  adoptConversation(conversationId) {
    this.#currentId = conversationId;
  }

  activate() {
    super.activate();
    this.#list.focus({ preventScroll: true });
    void this.refresh();
  }

  handleKeyDown(event) {
    if (!this.#conversations.length) return false;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const delta = event.key === "ArrowDown" ? 1 : -1;
      this.#selectedIndex = (this.#selectedIndex + delta + this.#conversations.length) % this.#conversations.length;
      this.#updateSelection();
      return true;
    }
    if (event.key === "Enter" && this.#conversations[this.#selectedIndex]) {
      event.preventDefault();
      void this.openConversation(this.#conversations[this.#selectedIndex].id);
      return true;
    }
    return false;
  }

  #run(task) {
    const operation = this.#queue.then(task).catch((error) => {
      console.error("Unable to update local history:", error);
      return null;
    });
    this.#queue = operation;
    return operation;
  }

  #renderHistory() {
    this.#list.replaceChildren();
    if (!this.#conversations.length) {
      const empty = document.createElement("div");
      empty.className = "history-empty";
      empty.textContent = "暂无历史对话";
      this.#list.append(empty);
      return;
    }
    for (const [index, item] of this.#conversations.entries()) {
      const option = document.createElement("button");
      option.type = "button";
      option.className = "history-item";
      option.classList.toggle("is-selected", index === this.#selectedIndex);
      option.setAttribute("role", "option");
      option.setAttribute("aria-selected", String(index === this.#selectedIndex));
      const title = document.createElement("strong");
      title.textContent = item.title;
      const time = document.createElement("span");
      time.className = "history-time";
      const running = this.isRunning?.(item.id);
      if (running) {
        const dot = document.createElement("span");
        dot.className = "history-running";
        dot.setAttribute("aria-label", "正在后台生成");
        dot.title = "正在后台生成";
        time.textContent = "";
        time.append(dot);
        option.classList.add("is-running");
      }
      time.append(this.#dateFormatter.format(new Date(item.updated_at * 1000)));
      option.append(title, time);
      option.addEventListener("mouseenter", () => {
        this.#selectedIndex = index;
        this.#updateSelection();
      });
      option.addEventListener("click", () => void this.openConversation(item.id));
      this.#list.append(option);
    }
  }

  #updateSelection() {
    for (const [index, option] of [...this.#list.querySelectorAll(".history-item")].entries()) {
      option.classList.toggle("is-selected", index === this.#selectedIndex);
      option.setAttribute("aria-selected", String(index === this.#selectedIndex));
    }
  }

  async refresh() {
    return this.#run(async () => {
      this.#conversations = await invoke("list_conversations");
      this.#selectedIndex = Math.max(0, this.#conversations.findIndex((item) => item.id === this.#currentId));
      this.#renderHistory();
    });
  }

  async #ensureConversation(message, pinnedId) {
    // 已经明确属于某个会话时直接用，避免切换对话后写错地方
    if (pinnedId) return pinnedId;
    return this.#createConversation(message);
  }

  async #createConversation(message) {
    if (this.#currentId) return this.#currentId;
    const parts = message.parts || [];
    const text = parts.find((part) => part.text)?.text?.trim() || "";
    const title = text.split("\n")[0].slice(0, 32) || "新对话";
    const created = await invoke("create_conversation", { title });
    this.#currentId = created.id;
    return this.#currentId;
  }

  /** 落库属于某个会话的消息。
   *  `pinnedId` 指定时绑定到该会话，不受后续切换对话影响——
   *  用于「回复被中断后切走」这类收尾写入。 */
  async record(message, pinnedId = null) {
    return this.#run(async () => {
      const conversationId = await this.#ensureConversation(message, pinnedId);
      const parts = message.parts || [{ type: "markdown", text: message.content ?? message.text ?? "" }];
      return invoke("append_message", { conversationId, role: message.role, parts });
    });
  }

  async openConversation(conversationId) {
    this.#scroll.capture();
    this.#scroll.reset();
    this.#generation += 1;
    const expectedGeneration = this.#generation;
    return this.#run(async () => {
      const messages = await invoke("load_messages", { conversationId });
      if (expectedGeneration !== this.#generation) return;
      this.#currentId = conversationId;
      this.#clearConversation();
      for (const message of messages) {
        this.#appendMessage({ role: message.role, parts: message.parts });
      }
      // 已有历史渲染完，再把该会话正在跑的流接到末尾
      this.onEnter?.(conversationId);
      this.manager.close(this);
      this.#scroll.restore(conversationId);
    });
  }

  async loadLatest() {
    const expectedGeneration = this.#generation;
    return this.#run(async () => {
      this.#conversations = await invoke("list_conversations");
      if (expectedGeneration !== this.#generation) return;
      const latest = this.#conversations[0];
      if (!latest) return;
      this.#currentId = latest.id;
      const messages = await invoke("load_messages", { conversationId: this.#currentId });
      if (expectedGeneration !== this.#generation) return;
      this.#clearConversation();
      for (const message of messages) {
        this.#appendMessage({ role: message.role, parts: message.parts });
      }
      this.onEnter?.(this.#currentId);
      this.#scroll.restore(this.#currentId);
    });
  }

  newChat() {
    this.#scroll.capture();
    this.#scroll.reset();
    this.#generation += 1;
    this.#currentId = null;
    this.#clearConversation();
    if (this.isOpen) this.manager.close(this);
  }
}

export function initHistory(manager, options) {
  return new HistoryView({
    name: "history",
    manager,
    element: document.querySelector("#history-view"),
    toggleButton: document.querySelector("#history-button"),
    initialFocus: document.querySelector("#history-list"),
    ...options,
    // Esc 返回聊天后直接能继续打字（放在展开之后，避免被 options 覆盖）
    returnFocus: document.querySelector("#message-input"),
  });
}
