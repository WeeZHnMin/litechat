import { invoke } from "@tauri-apps/api/core";

export const DEFAULT_SERVER = {
  name: "filesystem",
  command: "npx",
  args: ["-y", "@modelcontextprotocol/server-filesystem", "."],
  env: {},
  enabled: true,
};

const PRESETS = {
  filesystem: {
    name: "filesystem",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-filesystem", "."],
    env: {},
    enabled: true,
  },
  fetch: {
    name: "fetch",
    command: "uvx",
    args: ["mcp-server-fetch"],
    env: {},
    enabled: true,
  },
  memory: {
    name: "memory",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-memory"],
    env: {},
    enabled: true,
  },
};

function isBuiltin(server) {
  return server.name === "shell";
}

function createRow(server, onChange, onRemove) {
  const row = document.createElement("div");
  row.className = "mcp-row";

  const head = document.createElement("div");
  head.className = "mcp-row-head";

  const name = document.createElement("input");
  name.className = "mcp-name";
  name.placeholder = "名称";
  name.value = server.name ?? "";
  name.spellcheck = false;

  const enabled = document.createElement("label");
  enabled.className = "mcp-enabled";
  const checkbox = document.createElement("input");
  checkbox.type = "checkbox";
  checkbox.checked = server.enabled !== false;
  enabled.append(checkbox, document.createTextNode("启用"));

  const remove = document.createElement("button");
  remove.type = "button";
  remove.className = "plain-button mcp-remove";
  remove.textContent = "移除";

  head.append(name, enabled, remove);

  const commandLine = document.createElement("div");
  commandLine.className = "mcp-command";
  const command = document.createElement("input");
  command.placeholder = "命令，如 npx / uvx / D:\\tools\\mcp.exe";
  command.value = server.command ?? "";
  command.spellcheck = false;
  const args = document.createElement("input");
  args.placeholder = "参数，空格分隔";
  args.value = (server.args ?? []).join(" ");
  args.spellcheck = false;
  commandLine.append(command, args);
  row.append(head, commandLine);

  const status = document.createElement("span");
  status.className = "mcp-status";
  status.textContent = "";
  row.append(status);

  const emit = () => {
    onChange({
      name: name.value.trim(),
      command: command.value.trim(),
      args: args.value.split(/\s+/).filter(Boolean),
      env: server.env ?? {},
      enabled: checkbox.checked,
    });
  };
  for (const input of [name, command, args]) input.addEventListener("input", emit);
  checkbox.addEventListener("change", emit);
  remove.addEventListener("click", () => onRemove(row));

  return { row, status };
}

export function initMcpSettings() {
  const list = document.querySelector("#mcp-list");
  const addButton = document.querySelector("#mcp-add");
  const saveButton = document.querySelector("#mcp-save");
  const refreshButton = document.querySelector("#mcp-refresh");
  const summary = document.querySelector("#mcp-summary");
  const help = document.querySelector("#mcp-help");
  const errorLabel = document.querySelector("#mcp-error");
  const view = document.querySelector("#settings-view");

  let servers = [];
  let builtinTools = 0;
  const rows = new Map();

  function setError(text) {
    errorLabel.hidden = !text;
    errorLabel.textContent = text || "";
  }

  function render() {
    list.replaceChildren();
    rows.clear();
    const builtin = document.createElement("p");
    builtin.className = "mcp-builtin-note";
    builtin.textContent = "内置 shell 工具（run_command / run_shell）始终启用，无需配置。";
    list.append(builtin);
    if (!servers.length) {
      const empty = document.createElement("p");
      empty.className = "field-hint";
      empty.textContent = "尚未添加外部 MCP 服务";
      list.append(empty);
      return;
    }
    for (const server of servers) {
      const entry = createRow(
        server,
        (next) => {
          const index = servers.indexOf(server);
          if (index >= 0) servers[index] = next;
        },
        () => {
          servers = servers.filter((item) => item !== server);
          render();
        },
      );
      rows.set(server.name || `${servers.indexOf(server)}`, entry);
      list.append(entry.row);
    }
  }

  function renderSummary(externalConnected, externalTools) {
    const total = builtinTools + externalTools;
    const parts = [`内置 shell ${builtinTools} 个`];
    if (externalConnected) {
      parts.push(`外部服务 ${externalTools} 个`);
    }
    summary.textContent = `当前 ${total} 个工具可用于对话（${parts.join(" · ")}）`;
    summary.classList.add("is-ok");
    summary.classList.remove("is-warn");
  }

  function applyStatuses(statuses) {
    const external = statuses.filter((item) => !isBuiltin(item));
    const connected = external.filter((item) => item.connected);
    const externalTools = connected.reduce((sum, item) => sum + item.toolCount, 0);
    renderSummary(connected.length > 0, externalTools);
    for (const entry of rows.values()) {
      const nameInput = entry.row.querySelector(".mcp-name");
      const status = external.find((item) => item.name === nameInput.value.trim());
      entry.row.classList.toggle("is-connected", Boolean(status?.connected));
      entry.row.classList.toggle("is-error", Boolean(status?.error));
      if (!status) {
        entry.status.textContent = "";
      } else if (status.error) {
        entry.status.textContent = status.error;
      } else if (status.connected) {
        entry.status.textContent = `${status.toolCount} 个工具可用`;
      } else {
        entry.status.textContent = status.enabled ? "未连接" : "已停用";
      }
    }
  }

  async function load() {
    try {
      const statuses = await invoke("get_mcp_servers");
      servers = statuses
        .filter((item) => !isBuiltin(item))
        .map((item) => ({
          name: item.name,
          command: item.command,
          args: item.args ?? [],
          env: item.env ?? {},
          enabled: item.enabled,
        }));
      builtinTools = statuses
        .filter(isBuiltin)
        .reduce((sum, item) => sum + item.toolCount, 0);
      render();
      applyStatuses(statuses);
    } catch (error) {
      setError(String(error));
    }
  }

  async function save() {
    setError("");
    saveButton.disabled = true;
    try {
      const statuses = await invoke("save_mcp_servers", { servers });
      applyStatuses(statuses);
    } catch (error) {
      setError(String(error));
    } finally {
      saveButton.disabled = false;
    }
  }

  async function refresh() {
    setError("");
    refreshButton.disabled = true;
    try {
      applyStatuses(await invoke("refresh_mcp"));
    } catch (error) {
      setError(String(error));
    } finally {
      refreshButton.disabled = false;
    }
  }

  addButton.addEventListener("click", () => {
    servers.push({
      ...DEFAULT_SERVER,
      name: `server${servers.length + 1}`,
      args: [...DEFAULT_SERVER.args],
      env: {},
    });
    render();
  });

  const presetBox = document.querySelector("#mcp-presets");
  presetBox.addEventListener("click", (event) => {
    const button = event.target.closest(".mcp-preset");
    if (!button) return;
    const key = button.dataset.preset;
    if (key === "custom") {
      help.open = true;
      help.scrollIntoView({ behavior: "smooth", block: "center" });
      return;
    }
    const preset = PRESETS[key];
    if (!preset) return;
    const name = servers.some((item) => item.name === preset.name)
      ? `${preset.name}${servers.length + 1}`
      : preset.name;
    servers.push({ ...preset, name, args: [...preset.args], env: {} });
    render();
  });
  saveButton.addEventListener("click", save);
  refreshButton.addEventListener("click", refresh);

  new MutationObserver(() => {
    if (!view.hidden) void load();
  }).observe(view, { attributes: true, attributeFilter: ["hidden"] });

  window.addEventListener("litechat:mcp-changed", () => void load());

  void load();

  return { reload: load };
}
