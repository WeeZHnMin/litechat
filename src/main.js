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
function renderMessage(message, options = {}) {
  return appendMessage(message, {
    onResend: (text) => composer.setText(text),
    ...options,
  });
}

let modelSettings = null;
const composer = initComposer({
  appendMessage: renderMessage,
  onStop: () => runner.stop(),
  onReasoningChange: (effort) => modelSettings?.setReasoningEffort(effort),
  onModelChange: (name) => modelSettings?.setActiveModel(name),
  onListModels: () => invoke("list_saved_models"),
});
let runningQuery = () => false;
const history = initHistory(manager, {
  appendMessage: renderMessage,
  clearConversation,
  conversationElement: document.querySelector("#conversation"),
  // runner 在下面创建，这里延迟取用
  isRunning: (id) => Boolean(runningQuery(id)),
});
const runner = createConversationRunner({
  history,
  getConversationId: () => history.currentId,
  getContainer: () => document.querySelector("#conversation"),
  onBusyChange: (busy) => composer.setBusy(busy),
  // 某个会话开始 / 结束后台生成时，刷新历史列表的标记
  onRunningChange: () => void history.refresh(),
  onPendingSettled: (entries) => {
    for (const entry of entries) {
      entry.rendered?.markUndelivered?.((text) => void sendText(text));
    }
  },
});

runningQuery = (id) => runner.isRunning(id);

/** 重新发送一条未送达的消息。 */
async function sendText(text) {
  if (!text) return;
  const message = { role: "user", parts: [{ type: "text", text }] };
  renderMessage(message);
  await runner.send(message);
}
// 切进某个会话：把它正在后台跑的流接到历史记录末尾
history.onEnter = (conversationId) => runner.switchTo(conversationId);
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
  onReasoningChange: (effort) => composer.setReasoningEffort(effort),
  onModelChange: (name) => composer.setModel(name),
});
initMcpSettings();
initWorkspaceSettings();
initApprovalDialog();
initPermissionToggle();

document.addEventListener("keydown", (event) => manager.handleKeyDown(event), true);

function resetChat() {
  runner.leave();
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

window.addEventListener("litechat:send", async (event) => {
  const { message, rendered } = event.detail;
  if (!window.__TAURI_INTERNALS__) {
    void history.record(message);
    return;
  }
  // 仅在「当前正在看、且它自己还有流在跑」时才走插话
  if (runner.hasActiveStream()) {
    const queued = await runner.queueDuringStream(message, rendered);
    if (queued) return;
  }
  void runner.send(message);
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
