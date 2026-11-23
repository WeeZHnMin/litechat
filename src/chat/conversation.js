import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { createAssistantStream } from "./chat";

export function createConversationRunner({ history, getConversationId, onBusyChange }) {
  let current = null;

  function isTauri() {
    return Boolean(window.__TAURI_INTERNALS__);
  }

  async function ensureConversation(text) {
    const existing = getConversationId();
    if (existing) return existing;
    const title = text.split("\n")[0].slice(0, 32) || "新对话";
    const created = await invoke("create_conversation", { title });
    history.adoptConversation(created.id);
    return created.id;
  }

  function setBusy(stream) {
    current = stream;
    onBusyChange?.(Boolean(stream), stream?.streamId ?? null);
  }

  async function send(text) {
    if (current || !isTauri()) return;
    let conversationId;
    try {
      conversationId = await ensureConversation(text);
    } catch (error) {
      console.error("Unable to create conversation:", error);
      return;
    }
    const stream = createAssistantStream();
    let streamId = null;
    let settled = false;

    const unlisten = [];
    const cleanup = () => {
      for (const off of unlisten) off();
      unlisten.length = 0;
      if (streamId) void invoke("cancel_stream", { streamId }).catch(() => {});
    };

    const finish = (options) => {
      if (settled) return;
      settled = true;
      cleanup();
      const output = stream.done(options);
      const thoughts = stream.reasoning.trim();
      const seconds = stream.reasoningSeconds;
      const tools = stream.toolCalls.map((block) => ({ ...block.record }));
      setBusy(null);
      if (!output.trim() && !thoughts && !tools.length) return;
      const parts = [];
      if (thoughts) parts.push({ type: "reasoning", text: thoughts, seconds });
      for (const tool of tools) parts.push({ type: "tool", ...tool });
      if (output.trim()) parts.push({ type: "markdown", text: output });
      void history.record({ role: "assistant", parts });
    };

    const fail = (message) => {
      if (settled) return;
      settled = true;
      cleanup();
      stream.fail(new Error(message));
      setBusy(null);
    };

    unlisten.push(
      await listen("chat://reasoning", (event) => {
        if (event.payload?.streamId !== streamId) return;
        stream.pushReasoning(event.payload.text);
      }),
    );
    unlisten.push(
      await listen("chat://tool-call", (event) => {
        if (event.payload?.streamId !== streamId) return;
        stream.showToolCall(event.payload);
        const name = event.payload?.name;
        if (name === "add_mcp_server" || name === "remove_mcp_server") {
          window.dispatchEvent(new CustomEvent("litechat:mcp-changed"));
        }
      }),
    );
    unlisten.push(
      await listen("chat://delta", (event) => {
        if (event.payload?.streamId !== streamId) return;
        stream.push(event.payload.text);
      }),
    );
    unlisten.push(
      await listen("chat://done", (event) => {
        if (event.payload?.streamId !== streamId) return;
        finish();
      }),
    );
    unlisten.push(
      await listen("chat://error", (event) => {
        if (event.payload?.streamId !== streamId) return;
        fail(event.payload.message);
      }),
    );

    try {
      streamId = await invoke("send_chat", { conversationId, text });
    } catch (error) {
      fail(String(error));
      return;
    }
    if (settled) {
      void invoke("cancel_stream", { streamId }).catch(() => {});
      return;
    }
    setBusy({ streamId, stop: () => finish({ markInterrupted: true }) });
  }

  function stop() {
    current?.stop();
  }

  return {
    send,
    stop,
    get isBusy() {
      return Boolean(current);
    },
  };
}
