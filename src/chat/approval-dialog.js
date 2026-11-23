import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const LABELS = {
  run_command: { title: "执行 PowerShell 命令", risk: "会在本机运行任意命令，可能修改或删除文件" },
  run_shell: { title: "执行 cmd 命令", risk: "会在本机运行任意命令，可能修改或删除文件" },
  write_file: { title: "写入文件", risk: "会覆盖目标文件的全部内容" },
  edit_file: { title: "修改文件", risk: "会改写目标文件中的一段内容" },
  add_mcp_server: { title: "添加 MCP 服务", risk: "会启动一个新的本机进程并挂载它的工具" },
  remove_mcp_server: { title: "移除 MCP 服务", risk: "会停用该服务的全部工具" },
};

function describe(tool) {
  return LABELS[tool] ?? { title: `调用 ${tool}`, risk: "外部服务提供的工具" };
}

function summarize(tool, args) {
  if (tool === "run_command" || tool === "run_shell") {
    return { label: "将要执行", value: args.command ?? "" };
  }
  if (tool === "write_file") {
    const size = (args.content ?? "").length;
    const lines = (args.content ?? "").split("\n").length;
    return {
      label: `将要写入 ${args.path ?? ""}`,
      value: `（${lines} 行，约 ${size} 个字符）\n\n${(args.content ?? "").slice(0, 400)}`,
    };
  }
  if (tool === "edit_file") {
    return {
      label: `将在 ${args.path ?? ""} 中替换`,
      value: `原文：\n${(args.old_text ?? "").slice(0, 300)}\n\n改为：\n${(args.new_text ?? "").slice(0, 300)}`,
    };
  }
  if (tool === "add_mcp_server") {
    const argv = Array.isArray(args.args) ? args.args.join(" ") : "";
    return {
      label: `将要启动 ${args.name ?? ""}`,
      value: `${args.command ?? ""} ${argv}`.trim(),
    };
  }
  if (tool === "remove_mcp_server") {
    return { label: "将要移除", value: args.name ?? "" };
  }
  return { label: "参数", value: JSON.stringify(args, null, 2) };
}

export function initApprovalDialog() {
  if (!window.__TAURI_INTERNALS__) return { close() {} };

  const overlay = document.createElement("div");
  overlay.className = "approval-overlay";
  overlay.hidden = true;
  overlay.setAttribute("role", "dialog");
  overlay.setAttribute("aria-modal", "true");
  overlay.setAttribute("aria-label", "工具调用确认");

  overlay.innerHTML = `
    <div class="approval-card">
      <h2 class="approval-title"></h2>
      <p class="approval-risk"></p>
      <p class="approval-subtitle"></p>
      <p class="approval-label"></p>
      <pre class="approval-body"></pre>
      <div class="approval-actions">
        <button class="plain-button" data-action="deny" type="button">拒绝</button>
        <button class="plain-button" data-action="allow" type="button">允许一次</button>
        <button class="primary-button" data-action="always" type="button">此工具不再询问</button>
      </div>
    </div>
  `;
  document.body.append(overlay);

  const title = overlay.querySelector(".approval-title");
  const risk = overlay.querySelector(".approval-risk");
  const subtitle = overlay.querySelector(".approval-subtitle");
  const label = overlay.querySelector(".approval-label");
  const body = overlay.querySelector(".approval-body");
  let pending = null;

  function close() {
    overlay.hidden = true;
    pending = null;
  }

  async function respond(allowed, remember) {
    if (!pending) return;
    const { id, tool } = pending;
    close();
    try {
      await invoke("respond_approval", { id, allowed, remember, tool });
    } catch (error) {
      console.error("Unable to respond to approval:", error);
    }
  }

  overlay.addEventListener("click", (event) => {
    if (event.target === overlay) {
      void respond(false, null);
      return;
    }
    const button = event.target.closest("[data-action]");
    if (!button) return;
    const action = button.dataset.action;
    if (action === "deny") void respond(false, null);
    else if (action === "allow") void respond(true, null);
    else if (action === "always") void respond(true, "session");
  });

  document.addEventListener("keydown", (event) => {
    if (overlay.hidden || event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    void respond(false, null);
  }, true);

  void listen("chat://approval-request", (event) => {
    const payload = event.payload ?? {};
    pending = { id: payload.id, tool: payload.tool };
    let args = {};
    try {
      args = JSON.parse(payload.arguments ?? "{}");
    } catch {
      args = {};
    }
    const meta = describe(payload.tool);
    const detail = summarize(payload.tool, args);
    title.textContent = meta.title;
    risk.textContent = meta.risk;
    subtitle.textContent = `工具 ${payload.server}__${payload.tool} · 模型请求执行`;
    label.textContent = detail.label;
    body.textContent = detail.value;
    overlay.hidden = false;
    overlay.querySelector('[data-action="allow"]').focus();
  });

  return { close, get isOpen() { return !overlay.hidden; } };
}
