import { copyText } from "../ui/clipboard";
import { showContextMenu } from "../ui/context-menu";
import { renderMarkdown } from "../render/markdown";
import { renderCitationsPart, renderFilePart, renderImagePart } from "../render/media";

function getConversation() {
  return document.querySelector("#conversation");
}

function shouldStickToBottom() {
  const conversation = getConversation();
  return conversation.scrollHeight - conversation.scrollTop - conversation.clientHeight < 80;
}

function scrollToBottom(behavior = "smooth") {
  const conversation = getConversation();
  conversation.scrollTo({ top: conversation.scrollHeight, behavior });
}

function normalizeParts(message) {
  if (Array.isArray(message.parts)) return message.parts;
  return [{ type: "markdown", text: message.content ?? message.text ?? "" }];
}

function createReasoningBlock() {
  const wrapper = document.createElement("details");
  wrapper.className = "reasoning-block";
  wrapper.open = true;
  const summary = document.createElement("summary");
  summary.textContent = "思考中";
  const body = document.createElement("div");
  body.className = "reasoning-body";
  wrapper.append(summary, body);
  return {
    element: wrapper,
    setText(value) {
      body.textContent = value;
    },
    finish(seconds) {
      wrapper.open = false;
      summary.textContent = seconds ? `思考完成 · 用时 ${seconds} 秒` : "思考完成";
    },
  };
}

function createToolBlock() {
  const wrapper = document.createElement("details");
  wrapper.className = "tool-block";
  const summary = document.createElement("summary");
  const label = document.createElement("span");
  label.className = "tool-label";
  const state = document.createElement("span");
  state.className = "tool-state";
  summary.append(label, state);
  const body = document.createElement("div");
  body.className = "tool-body";
  wrapper.append(summary, body);
  const record = { server: "", name: "", arguments: "", result: null, error: null };
  return {
    element: wrapper,
    record,
    setArguments(text) {
      record.arguments = text || "";
      body.textContent = `参数：${text || "{}"}`;
    },
    setResult(text, error) {
      wrapper.classList.add(error ? "is-error" : "is-done");
      state.textContent = error ? "失败" : "完成";
      record.result = text ?? null;
      record.error = error ?? null;
      if (text) {
        const result = document.createElement("pre");
        result.className = "tool-result";
        result.textContent = text;
        body.append(result);
      }
    },
    describe(server, name) {
      record.server = server || "mcp";
      record.name = name || "tool";
      label.textContent = `${record.server} · ${record.name}`;
      state.textContent = "调用中…";
    },
  };
}

function answerAnchor(content) {
  const body = content.querySelector(".markdown-body");
  if (!body) return content;
  const block = [...body.children].find((child) => !child.classList.contains("reasoning-block"));
  return block ?? body;
}

function scrollToAnswerStart(content) {
  requestAnimationFrame(() => {
    const conversation = getConversation();
    const anchor = answerAnchor(content);
    const delta =
      anchor.getBoundingClientRect().top - conversation.getBoundingClientRect().top;
    const target = Math.max(0, conversation.scrollTop + delta - 24);
    conversation.scrollTo({ top: target, behavior: "smooth" });
  });
}

function renderPart(part, role) {
  if (["markdown", "text"].includes(part.type)) {
    if (role === "user") {
      const text = document.createElement("div");
      text.className = "user-message-text";
      text.textContent = part.text || "";
      return text;
    }
    return renderMarkdown(part.text);
  }
  if (part.type === "reasoning") {
    const block = createReasoningBlock();
    block.setText(part.text || "");
    block.finish(part.seconds);
    return block.element;
  }
  if (part.type === "tool") {
    const block = createToolBlock();
    block.describe(part.server || "mcp", part.name || "tool");
    block.setArguments(part.arguments);
    block.setResult(part.result, part.error);
    return block.element;
  }
  if (part.type === "image") return renderImagePart(part);
  if (part.type === "file") return renderFilePart(part);
  if (part.type === "citations") return renderCitationsPart(part);
  return null;
}

export function messageText(message) {
  return normalizeParts(message)
    .filter((part) => ["markdown", "text"].includes(part.type))
    .map((part) => part.text || "")
    .join("\n\n")
    .trim();
}

function attachMessageMenu(element, message, extraItems = []) {
  element.addEventListener("contextmenu", (event) => {
    if (event.target.closest(".code-block, .image-preview-button, img, a, .file-card, .citation-card")) {
      return;
    }
    event.preventDefault();
    const selection = window.getSelection()?.toString().trim() ?? "";
    const text = selection || messageText(message);
    showContextMenu(event.clientX, event.clientY, [
      selection ? { label: "复制选中内容", action: () => copyText(selection) } : null,
      text ? { label: "复制整条消息", action: () => copyText(text) } : null,
      ...extraItems,
    ]);
  });
}

export function appendMessage(message, { onResend } = {}) {
  const role = message.role === "user" ? "user" : "assistant";
  const element = document.createElement("article");
  element.className = `message message--${role}`;
  const content = document.createElement("div");
  content.className = "message-content";
  for (const part of normalizeParts(message)) {
    const rendered = renderPart(part, role);
    if (rendered) content.append(rendered);
  }
  element.append(content);
  const text = messageText(message);
  const extraItems = [];
  if (role === "user" && onResend && text) {
    extraItems.push({ separator: true }, { label: "重新发送", action: () => onResend(text) });
  }
  attachMessageMenu(element, message, extraItems);
  getConversation().append(element);
  scrollToBottom();
  return { element, content };
}

export function createAssistantStream() {
  let text = "";
  let reasoning = "";
  let timer = 0;
  let frame = 0;
  let lastRenderAt = 0;
  let finished = false;
  const MIN_RENDER_INTERVAL = 50;
  const { element, content } = appendMessage({ role: "assistant", parts: [] });
  element.classList.add("is-streaming");
  let reasoningBlock = null;
  let reasoningStartedAt = 0;
  const toolBlocks = new Map();
  let toolSeq = 0;

  function rememberToolCall(payload) {
    const key = payload.id || `tool_${toolSeq++}`;
    const existing = toolBlocks.get(key);
    if (existing) {
      existing.setResult(payload.result, payload.error);
      return;
    }
    const block = createToolBlock();
    block.describe(payload.server, payload.name);
    block.setArguments(payload.arguments);
    if (payload.phase === "done") block.setResult(payload.result, payload.error);
    toolBlocks.set(key, block);
    if (content.querySelector(".markdown-body")) {
      content.append(block.element);
    } else {
      render(false);
    }
    if (!frame) {
      const stick = shouldStickToBottom();
      if (stick) scrollToBottom("auto");
    }
  }

  const render = (enhance) => {
    const stick = shouldStickToBottom();
    content.replaceChildren(renderMarkdown(text, { enhance }));
    if (reasoningBlock) content.prepend(reasoningBlock.element);
    for (const block of toolBlocks.values()) content.append(block.element);
    if (stick) scrollToBottom("auto");
  };

  const cancelPending = () => {
    if (timer) {
      clearTimeout(timer);
      timer = 0;
    }
    if (frame) {
      cancelAnimationFrame(frame);
      frame = 0;
    }
  };

  const flush = () => {
    frame = 0;
    if (finished) return;
    lastRenderAt = performance.now();
    render(false);
  };

  const queueRender = () => {
    if (finished || timer || frame) return;
    const wait = Math.max(0, MIN_RENDER_INTERVAL - (performance.now() - lastRenderAt));
    if (wait > 0) {
      timer = window.setTimeout(() => {
        timer = 0;
        if (finished) return;
        frame = requestAnimationFrame(flush);
      }, wait);
    } else {
      frame = requestAnimationFrame(flush);
    }
  };

  return {
    element,
    push(chunk) {
      if (finished) return;
      text += chunk;
      queueRender();
    },
    pushReasoning(chunk) {
      if (finished) return;
      if (!reasoningBlock) {
        reasoningBlock = createReasoningBlock();
        reasoningStartedAt = performance.now();
        content.prepend(reasoningBlock.element);
      }
      reasoning += chunk;
      reasoningBlock.setText(reasoning);
      if (!frame) {
        const stick = shouldStickToBottom();
        if (stick) scrollToBottom("auto");
      }
    },
    get isFinished() {
      return finished;
    },
    get text() {
      return text;
    },
    get reasoning() {
      return reasoning;
    },
    showToolCall(payload) {
      if (finished) return;
      rememberToolCall(payload);
    },
    get reasoningSeconds() {
      if (!reasoningStartedAt) return null;
      return Math.max(1, Math.round((performance.now() - reasoningStartedAt) / 1000));
    },
    get toolCalls() {
      return Array.from(toolBlocks.values());
    },
    done({ markInterrupted = false } = {}) {
      if (finished) return text;
      finished = true;
      cancelPending();
      if (reasoningBlock) {
        const seconds = reasoningStartedAt
          ? Math.max(1, Math.round((performance.now() - reasoningStartedAt) / 1000))
          : null;
        reasoningBlock.finish(seconds);
      }
      render(true);
      element.classList.remove("is-streaming");
      if (markInterrupted) element.classList.add("message--interrupted");
      scrollToAnswerStart(content);
      return text;
    },
    fail(error) {
      finished = true;
      cancelPending();
      element.classList.remove("is-streaming");
      element.classList.add("message--error");
      content.replaceChildren(renderMarkdown(error?.message || "回复生成失败，请稍后重试。", { enhance: false }));
    },
  };
}

export function clearConversation() {
  getConversation().replaceChildren();
}
