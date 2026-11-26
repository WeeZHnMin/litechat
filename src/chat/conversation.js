import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { createAssistantStream } from "./chat";

export function createConversationRunner({
  history,
  getConversationId,
  onBusyChange,
  onQueuedChange,
  onPendingSettled,
}) {
  let current = null;
  /** 生成中收到的消息先排在这里，当前轮结束后按序发出。 */
  let pending = [];

  function isTauri() {
    return Boolean(window.__TAURI_INTERNALS__);
  }

  function summarize(message) {
    const parts = message?.parts ?? [];
    const text = parts.find((part) => part.type === "text")?.text ?? "";
    const firstLine = text.trim().split("\n")[0].slice(0, 32);
    if (firstLine) return firstLine;
    const image = parts.find((part) => part.type === "image");
    return image ? "图片" : "新对话";
  }

  async function ensureConversation(message) {
    const existing = getConversationId();
    if (existing) return existing;
    const created = await invoke("create_conversation", { title: summarize(message) });
    history.adoptConversation(created.id);
    return created.id;
  }

  function setBusy(stream) {
    current = stream;
    onBusyChange?.(Boolean(stream), stream?.streamId ?? null);
  }

  /**
   * 把这一轮落库。没被插话时就是一条助手消息；
   * 被插话过则按「助手段 → 用户消息 → 助手段 …」的顺序还原。
   */
  async function persistSplit(stream, parts, interleaved = []) {
    const segments = stream?.segments;
    if (!segments || segments.length <= 1 || !interleaved.length) {
      await history.record({ role: "assistant", parts });
      return;
    }
    for (const [index, segment] of segments.entries()) {
      if (segment.length) {
        await history.record({ role: "assistant", parts: segment });
      }
      // 只有真正被模型读到的消息才写进历史，没送达的留待用户重发
      const entry = interleaved[index];
      if (entry?.accepted) {
        await history.record(entry.message);
      }
    }
  }

  async function send(message) {
    if (!isTauri()) return;
    // 生成中：先排队，由 finish/stop 负责接续，不在中途另开一条流
    if (current) {
      pending.push(message);
      onQueuedChange?.(pending.length);
      return;
    }
    await run(message);
  }

  /** 生成中再发一条：打断当前轮，把新消息插进来立即处理。 */

  async function drainQueue() {
    if (current || !pending.length) return;
    const next = pending.shift();
    onQueuedChange?.(pending.length);
    await run(next);
  }

  async function run(message) {
    let conversationId;
    try {
      conversationId = await ensureConversation(message);
      await history.record(message);
    } catch (error) {
      console.error("Unable to store user message:", error);
      return;
    }
    const stream = createAssistantStream();
    let streamId = null;
    let settled = false;
    /** 本轮里被用户插话的消息，按插入顺序排列。 */
    const interleaved = [];

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
      stream.done(options);
      const parts = stream.parts;
      setBusy(null);
      // 这一轮结束时还挂着「等待读取」的消息，说明它们赶在最后一轮之后发出，
      // 不会再被模型看到。标记成未送达，让用户自己决定是否重发。
      onPendingSettled?.(interleaved.filter((entry) => !entry.accepted));
      if (parts.length) {
        // 被用户插话打断过的话，按「助手段 → 用户消息 → 助手段」的顺序落库，
        // 重新加载才能和界面显示一致
        void persistSplit(stream, parts, interleaved).then(() => drainQueue());
      } else {
        void drainQueue();
      }
    };

    const fail = (message) => {
      if (settled) return;
      settled = true;
      cleanup();
      stream.fail(new Error(message));
      setBusy(null);
      void drainQueue();
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
      await listen("chat://input-accepted", (event) => {
        if (event.payload?.streamId !== streamId) return;
        // 按发送顺序认领：最早一条未被确认的就是它
        const entry = interleaved.find((item) => !item.accepted);
        if (entry) entry.accepted = true;
        window.dispatchEvent(
          new CustomEvent("litechat:input-accepted", { detail: event.payload }),
        );
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
      streamId = await invoke("send_chat", { conversationId });
    } catch (error) {
      fail(String(error));
      return;
    }
    if (settled) {
      void invoke("cancel_stream", { streamId }).catch(() => {});
      return;
    }
    setBusy({
      streamId,
      stream,
      interleaved,
      stop: () => finish({ markInterrupted: true }),
    });
  }

  /** 生成中再发一条：交给后端，在下一段思考开始前注入。 */
  async function queueDuringStream(message, rendered) {
    if (!current || !isTauri()) return false;
    const text = (message.parts ?? [])
      .filter((part) => ["text", "markdown"].includes(part.type))
      .map((part) => part.text ?? "")
      .join("\n\n")
      .trim();
    if (!text) return false;
    try {
      await invoke("queue_user_input", { streamId: current.streamId, text });
      // 用户消息此刻已经上屏，把助手这一轮断开，后续内容才会排在它下面
      current.stream?.split?.();
      // 记下插在第几段之后，落库时按同样顺序还原
      current.interleaved.push({ message, rendered });
      return true;
    } catch (error) {
      console.error("Unable to queue message:", error);
      return false;
    }
  }

  /** 用户点「停止」：中断当前轮，队列里的消息照常继续。 */
  function stop() {
    current?.stop();
  }

  /** 用户想立刻发新消息：丢掉未发出的排队项，只发这一条。 */
  async function sendNow(message) {
    if (!isTauri()) return;
    if (!current) {
      await run(message);
      return;
    }
    pending = [message];
    onQueuedChange?.(pending.length);
    current.stop();
  }

  function clearQueue() {
    pending = [];
    onQueuedChange?.(0);
  }

  return {
    send,
    sendNow,
    queueDuringStream,
    stop,
    clearQueue,
    get isBusy() {
      return Boolean(current);
    },
    get queuedCount() {
      return pending.length;
    },
  };
}
