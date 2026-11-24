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

function scrollToAnchor(element) {
  requestAnimationFrame(() => {
    const conversation = getConversation();
    const delta =
      element.getBoundingClientRect().top - conversation.getBoundingClientRect().top;
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
  let tools = null;
  for (const part of normalizeParts(message)) {
    if (part.type === "tool") {
      if (!tools) {
        tools = [];
        content.append(tools);
      }
      const block = createToolBlock();
      block.describe(part.server || "mcp", part.name || "tool");
      block.setArguments(part.arguments);
      block.setResult(part.result, part.error);
      tools.push(block.element);
      continue;
    }
    tools = null;
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
  let timer = 0;
  let frame = 0;
  let lastRenderAt = 0;
  let finished = false;
  const MIN_RENDER_INTERVAL = 50;
  const { element, content } = appendMessage({ role: "assistant", parts: [] });
  element.classList.add("is-streaming");

  const timeline = [];
  const toolSeq = { value: 0 };
  const toolIndex = new Map();
  let reasoningEntry = null;
  let textEntry = null;

  function newTextEntry() {
    const markdown = document.createElement("div");
    markdown.className = "markdown-body";
    content.append(markdown);
    const entry = { kind: "text", element: markdown, text: "" };
    timeline.push(entry);
    return entry;
  }

  function reasoningChunk(chunk) {
    if (!reasoningEntry) {
      const block = createReasoningBlock();
      content.append(block.element);
      reasoningEntry = {
        kind: "reasoning",
        element: block.element,
        block,
        text: "",
        startedAt: performance.now(),
        seconds: null,
      };
      timeline.push(reasoningEntry);
    }
    reasoningEntry.text += chunk;
    reasoningEntry.block.setText(reasoningEntry.text);
    if (!frame) {
      const stick = shouldStickToBottom();
      if (stick) scrollToBottom("auto");
    }
  }

  function textChunk(chunk) {
    if (!textEntry) textEntry = newTextEntry();
    textEntry.text += chunk;
    queueRender();
  }

  function closeReasoning() {
    if (!reasoningEntry || reasoningEntry.seconds !== null) return;
    reasoningEntry.seconds = reasoningEntry.startedAt
      ? Math.max(1, Math.round((performance.now() - reasoningEntry.startedAt) / 1000))
      : null;
    reasoningEntry.block.finish(reasoningEntry.seconds);
  }

  function rememberToolCall(payload) {
    const key = payload.id || `tool_${toolSeq.value++}`;
    const existing = toolIndex.get(key);
    if (existing) {
      existing.setResult(payload.result, payload.error);
      if (!frame) {
        const stick = shouldStickToBottom();
        if (stick) scrollToBottom("auto");
      }
      return;
    }
    closeReasoning();
    reasoningEntry = null;
    textEntry = null;

    const block = createToolBlock();
    block.describe(payload.server, payload.name);
    block.setArguments(payload.arguments);
    if (payload.phase === "done") block.setResult(payload.result, payload.error);
    content.append(block.element);
    timeline.push({ kind: "tool", element: block.element, record: block.record });
    toolIndex.set(key, block);

    if (!frame) {
      const stick = shouldStickToBottom();
      if (stick) scrollToBottom("auto");
    }
  }

  const render = (enhance) => {
    const stick = shouldStickToBottom();
    if (textEntry) textEntry.element.replaceChildren(renderMarkdown(textEntry.text, { enhance }));
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

  const answerText = () =>
    timeline
      .filter((entry) => entry.kind === "text")
      .map((entry) => entry.text)
      .join("");
  const allReasoning = () =>
    timeline
      .filter((entry) => entry.kind === "reasoning")
      .map((entry) => entry.text)
      .join("");
  const allTools = () => timeline.filter((entry) => entry.kind === "tool");

  return {
    element,
    push(chunk) {
      if (finished) return;
      textChunk(chunk);
    },
    pushReasoning(chunk) {
      if (finished) return;
      reasoningChunk(chunk);
    },
    get isFinished() {
      return finished;
    },
    get text() {
      return answerText();
    },
    get reasoning() {
      return allReasoning();
    },
    showToolCall(payload) {
      if (finished) return;
      rememberToolCall(payload);
    },
    get reasoningSeconds() {
      const withSeconds = timeline
        .filter((entry) => entry.kind === "reasoning" && entry.seconds)
        .map((entry) => entry.seconds);
      return withSeconds.length ? withSeconds.reduce((a, b) => a + b, 0) : null;
    },
    get toolCalls() {
      return allTools().map((entry) => entry.element);
    },
    get parts() {
      return timeline
        .map((entry) => {
          if (entry.kind === "text") {
            return entry.text.trim()
              ? { type: "markdown", text: entry.text }
              : null;
          }
          if (entry.kind === "reasoning") {
            return entry.text.trim()
              ? { type: "reasoning", text: entry.text, seconds: entry.seconds }
              : null;
          }
          return { type: "tool", ...entry.record };
        })
        .filter(Boolean);
    },
    done({ markInterrupted = false } = {}) {
      if (finished) return answerText();
      finished = true;
      cancelPending();
      closeReasoning();
      render(true);
      element.classList.remove("is-streaming");
      if (markInterrupted) element.classList.add("message--interrupted");
      const firstText = timeline.find((entry) => entry.kind === "text");
      scrollToAnchor(firstText?.element ?? content);
      return answerText();
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
