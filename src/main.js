import "katex/dist/katex.min.css";
import "highlight.js/styles/github-dark.css";
import { listen } from "@tauri-apps/api/event";
import "./styles/styles.css";
import "./styles/messages.css";
import { invoke } from "@tauri-apps/api/core";
import { appendMessage, clearConversation, createAssistantStream } from "./chat/chat";
import { initApprovalDialog } from "./chat/approval-dialog";
import { initPermissionToggle } from "./chat/permission-toggle";
import { initComposer } from "./chat/composer";
import { installGlobalContextMenuGuard } from "./ui/context-menu";
import { createConversationRunner } from "./chat/conversation";
import { initHistory } from "./chat/history";
import { initMcpSettings } from "./settings/mcp-settings";
import { initWorkspaceSettings } from "./settings/workspace-settings";
import { initModelSettings } from "./settings/model-settings";
import { initSettings } from "./settings/settings";
import { renderShell } from "./app/shell";
import { ChatView, ViewManager } from "./app/views";
import { initWindowControls } from "./app/window-controls";

renderShell();
installGlobalContextMenuGuard((target) => Boolean(target.closest("input, textarea")));

const manager = new ViewManager({
  conversation: document.querySelector("#conversation"),
  composer: document.querySelector("#composer"),
});
function renderMessage(message) {
  return appendMessage(message, { onResend: (text) => composer.setText(text) });
}

let modelSettings = null;
const composer = initComposer({
  appendMessage: renderMessage,
  onStop: () => runner.stop(),
  onThinkingChange: (enabled) => void modelSettings?.setThinking(enabled),
});
const history = initHistory(manager, {
  appendMessage: renderMessage,
  clearConversation,
  conversationElement: document.querySelector("#conversation"),
});
const runner = createConversationRunner({
  history,
  getConversationId: () => history.currentId,
  onBusyChange: (busy) => composer.setBusy(busy),
});
history.onLeave = () => runner.stop();
const settings = initSettings(manager);
manager.register(history).register(settings);
manager.setBaseline(
  new ChatView({
    name: "chat",
    onEscape: () => {
      if (window.__TAURI_INTERNALS__) void invoke("minimize_window");
    },
  }),
);
modelSettings = initModelSettings({
  onThinkingChange: (enabled) => composer.setThinking(enabled),
});
initMcpSettings();
initWorkspaceSettings();
initApprovalDialog();
initPermissionToggle();

document.addEventListener("keydown", (event) => manager.handleKeyDown(event), true);

function resetChat() {
  runner.stop();
  history.newChat();
  composer.reset();
}

initWindowControls({
  onNewChat: startNewChat,
});

function startNewChat() {
  resetChat();
  manager.closeActive();
  composer.focus();
}

window.addEventListener("litechat:send", (event) => {
  const { message, text } = event.detail;
  if (window.__TAURI_INTERNALS__) {
    if (text?.trim()) void runner.send(text);
    return;
  }
  void history.record(message);
});

window.liteChat = {
  appendMessage(message) {
    const rendered = renderMessage(message);
    void history.record(message);
    return rendered;
  },
  createAssistantStream() {
    const stream = createAssistantStream();
    return {
      element: stream.element,
      push: (chunk) => stream.push(chunk),
      fail: (error) => stream.fail(error),
      done() {
        const text = stream.done();
        void history.record({ role: "assistant", parts: [{ type: "markdown", text }] });
        return text;
      },
    };
  },
  clearConversation: resetChat,
};

if (window.__TAURI_INTERNALS__) {
  void listen("shortcut:new-chat", startNewChat);
  void listen("shortcut:history", () => manager.open(history));
}

if (new URLSearchParams(window.location.search).has("demo")) {
  import("./app/demo").then(({ loadDemo }) => loadDemo(renderMessage));
} else {
  void history.loadLatest();
}
