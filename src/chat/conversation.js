import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { createAssistantStream } from "./chat";

export function createConversationRunner({
  history,
  getConversationId,
  onBusyChange,
  onQueuedChange,
  onPendingSettled,
  onRunningChange,
  getContainer,
}) {
  /** 每个会话一条正在运行的流。切走时流继续跑，只是不再挂载到界面上。 */
  const running = new Map();
  /** 当前界面对应的会话；只有它的流会显示在 DOM 里。 */
  let activeId = null;

  function isTauri() {
    return Boolean(window.__TAURI_INTERNALS__);
  }

  function current() {
    return activeId ? running.get(activeId) ?? null : null;
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

  /** 记录某会话的运行状态；只有它是对外可见的会话时才通知界面。 */
  function updateBusy(conversationId) {
    if (conversationId !== activeId) return;
    const entry = running.get(conversationId);
    onBusyChange?.(Boolean(entry), entry?.streamId ?? null);
  }

  /**
   * 把这一轮落库。没被插话时就是一条助手消息；
   * 被插话过则按「助手段 → 用户消息 → 助手段 …」的顺序还原。
   */
  async function persistSplit(stream, parts, interleaved = [], pinnedId = null) {
    const segments = stream?.segments;
    if (!segments || segments.length <= 1 || !interleaved.length) {
      await history.record({ role: "assistant", parts }, pinnedId);
      return;
    }
    for (const [index, segment] of segments.entries()) {
      if (segment.length) {
        await history.record({ role: "assistant", parts: segment }, pinnedId);
      }
      const entry = interleaved[index];
      if (entry?.accepted) {
        await history.record(entry.message, pinnedId);
      }
    }
  }

  async function send(message) {
    if (!isTauri()) return;
    const conversationId = getConversationId();
    // 该会话已有流在跑：排队，等这一轮结束再发
    const entry = conversationId ? running.get(conversationId) : null;
    if (entry) {
      entry.pending.push(message);
      if (conversationId === activeId) onQueuedChange?.(entry.pending.length);
      return;
    }
    await run(message);
  }

  async function drainQueue(conversationId) {
    const entry = running.get(conversationId);
    if (!entry || !entry.pending.length) return;
    const next = entry.pending.shift();
    if (conversationId === activeId) onQueuedChange?.(entry.pending.length);
    running.delete(conversationId);
    await run(next, conversationId);
  }

  async function run(message, pinnedId = null) {
    let conversationId;
    try {
      conversationId = pinnedId ?? (await ensureConversation(message));
      // 固定写入本轮所属的会话，切换对话也不会写错地方
      await history.record(message, conversationId);
    } catch (error) {
      console.error("Unable to store user message:", error);
      return;
    }
    // 新会话或当前界面对应的就是它，才把元素挂到界面上
    if (!activeId || activeId === conversationId) activeId = conversationId;
    const visible = conversationId === activeId;
    const stream = createAssistantStream({ detached: !visible });
    /** 本轮里被用户插话的消息，按插入顺序排列。 */
    const interleaved = [];
    const entry = { stream, streamId: null, pending: [], stop: null, interleaved };
    running.set(conversationId, entry);
    onRunningChange?.(conversationId, true);
    let streamId = null;
    let settled = false;

    const unlisten = [];
    const cleanup = () => {
      // 单个监听器取消失败不能影响后续收尾（落库、清状态）
      for (const off of unlisten) {
        try {
          off();
        } catch (error) {
          console.error("Failed to unlisten:", error);
        }
      }
      unlisten.length = 0;
      if (streamId) void invoke("cancel_stream", { streamId }).catch(() => {});
    };

    const finish = (options) => {
      if (settled) return;
      settled = true;
      cleanup();
      stream.done(options);
      const parts = stream.parts;
      if (running.get(conversationId) === entry) {
        running.delete(conversationId);
        onRunningChange?.(conversationId, false);
      }
      // 后台跑完的流已落库，元素不必再挂回界面，避免与历史重复
      if (!stream.isMounted) stream.dispose?.();
      updateBusy(conversationId);
      // 结束时仍未被读到的消息标为未送达，由用户决定是否重发
      onPendingSettled?.(interleaved.filter((entry) => !entry.accepted));
      if (parts.length) {
        void persistSplit(stream, parts, interleaved, conversationId).then(() =>
          drainQueue(conversationId),
        );
      } else {
        void drainQueue(conversationId);
      }
    };

    const fail = (message) => {
      if (settled) return;
      settled = true;
      cleanup();
      stream.fail(new Error(message));
      if (running.get(conversationId) === entry) {
        running.delete(conversationId);
        onRunningChange?.(conversationId, false);
      }
      if (!stream.isMounted) stream.dispose?.();
      updateBusy(conversationId);
      void drainQueue(conversationId);
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
        const entry = interleaved.find((item) => !item.accepted);
        if (!entry) return;
        entry.accepted = true;
        entry.rendered?.markAccepted?.();
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
    entry.streamId = streamId;
    entry.stop = () => finish({ markInterrupted: true });
    updateBusy(conversationId);
  }

  /** 生成中再发一条：交给后端，在下一段思考开始前注入。 */
  async function queueDuringStream(message, rendered) {
    const entry = current();
    if (!entry || !isTauri()) return false;
    const text = (message.parts ?? [])
      .filter((part) => ["text", "markdown"].includes(part.type))
      .map((part) => part.text ?? "")
      .join("\n\n")
      .trim();
    if (!text) return false;
    try {
      await invoke("queue_user_input", { streamId: entry.streamId, text });
      // 断开当前轮，后续内容才会排在用户消息下面
      entry.stream?.split?.();
      entry.interleaved.push({ message, rendered });
      return true;
    } catch (error) {
      console.error("Unable to queue message:", error);
      return false;
    }
  }

  /** 用户点「停止」：中断当前轮，队列里的消息照常继续。 */
  function stop() {
    current()?.stop?.();
  }

  /** 切换对话：把当前流从界面上摘下来，但它继续在后台跑。 */
  function switchTo(conversationId) {
    const previous = activeId;
    if (previous === conversationId) return previous;

    // 摘掉旧会话的流（仍在内存里运行），挂上目标会话正在跑的流
    const leaving = previous ? running.get(previous) : null;
    if (leaving) leaving.stream?.unmount?.();

    activeId = conversationId;

    const entering = conversationId ? running.get(conversationId) : null;
    if (entering) entering.stream?.mount?.(getContainer?.());

    updateBusy(conversationId);
    onQueuedChange?.(entering?.pending.length ?? 0);
    return previous;
  }

  /** 离开当前对话（新建或切走）：摘下 DOM，流和它的队列都继续在后台跑。 */
  function leave() {
    const previous = activeId;
    const leaving = previous ? running.get(previous) : null;
    if (leaving) leaving.stream?.unmount?.();
    activeId = null;
    onQueuedChange?.(0);
    onBusyChange?.(false, null);
  }

  /** 用户想立刻发新消息：丢掉未发出的排队项，只发这一条。 */
  async function sendNow(message) {
    if (!isTauri()) return;
    const entry = current();
    if (!entry) {
      await run(message);
      return;
    }
    entry.pending = [message];
    onQueuedChange?.(1);
    entry.stop?.();
  }

  function clearQueue() {
    const entry = current();
    if (entry) entry.pending = [];
    onQueuedChange?.(0);
  }

  return {
    send,
    sendNow,
    queueDuringStream,
    stop,
    leave,
    switchTo,
    clearQueue,
    /** 当前正在看的会话是否还有流在跑。 */
    get isBusy() {
      return Boolean(current());
    },
    /** 同上，但语义更明确：能否把新消息插进正在生成的回复里。 */
    hasActiveStream() {
      const entry = current();
      return Boolean(entry?.streamId && !entry.stream?.isFinished);
    },
    get queuedCount() {
      return current()?.pending.length ?? 0;
    },
    /** 某会话是否正在后台运行，用于会话列表上打标。 */
    isRunning(conversationId) {
      return running.has(conversationId);
    },
    /** 所有正在后台运行的会话 id，用于列表标记。 */
    runningIds() {
      return [...running.keys()];
    },
  };
}
