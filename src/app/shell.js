const icons = {
  minimize: '<path d="M5 12h14" />',
  pin: '<path d="m9 3 6 6M12 3l9 9-4 1-5 5-1 4-9-9 4-1 5-5 1-4Z" /><path d="m8 16-5 5" />',
  newChat: '<path d="M12 20h9" /><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L8 18l-4 1 1-4Z" />',
  plus: '<path d="M12 5v14M5 12h14" />',
  settings: '<path d="M4 7h10M18 7h2M4 17h2M10 17h10" /><circle cx="16" cy="7" r="2" /><circle cx="8" cy="17" r="2" />',
  history: '<circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" />',
  back: '<path d="m15 18-6-6 6-6" />',
};

const icon = (name) => `
  <svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    ${icons[name]}
  </svg>
`;

export function renderShell() {
  document.querySelector("#app").innerHTML = `
    <main class="window-shell">
      <header class="titlebar" data-tauri-drag-region>
        <div class="titlebar-actions">
          <button class="icon-button" id="minimize" type="button" aria-label="最小化" title="最小化">${icon("minimize")}</button>
          <button class="icon-button" id="pin" type="button" aria-label="钉在桌面" aria-pressed="false" title="钉在桌面">${icon("pin")}</button>
          <button class="icon-button" id="new-chat" type="button" aria-label="新对话" title="新对话">${icon("newChat")}</button>
        </div>
        <div class="titlebar-actions titlebar-actions--right">
          <button class="icon-button" id="history-button" type="button" aria-label="历史会话" aria-controls="history-view" aria-expanded="false" title="历史会话">${icon("history")}</button>
          <button class="icon-button" id="settings-button" type="button" aria-label="设置" aria-controls="settings-view" aria-expanded="false" title="设置">${icon("settings")}</button>
        </div>
      </header>
      <section class="conversation" id="conversation" aria-label="聊天内容"></section>
      <section class="history-view" id="history-view" aria-label="历史会话" hidden>
        <header class="settings-header">
          <button class="icon-button" id="history-back" type="button" aria-label="返回聊天" title="返回聊天">${icon("back")}</button>
          <h1>历史会话</h1>
        </header>
        <p class="history-hint">↑ ↓ 选择 · Enter 打开 · Esc 返回</p>
        <div class="history-list" id="history-list" role="listbox" aria-label="历史会话列表" tabindex="0"></div>
      </section>
      <section class="settings-view" id="settings-view" aria-label="设置" hidden>
        <header class="settings-header">
          <button class="icon-button" id="settings-back" type="button" aria-label="返回聊天" title="返回聊天">${icon("back")}</button>
          <h1>设置</h1>
        </header>
        <section class="settings-section">
          <h2>全局快捷键</h2>
          <p>在 LiteChat 位于后台或托盘时同样有效。</p>
          <div class="setting-row">
            <div class="setting-copy"><strong>显示 / 隐藏窗口</strong><span>弹出、聚焦或隐藏到系统托盘</span></div>
            <button class="shortcut-key" id="toggle-shortcut" data-action="toggle" type="button">Alt+Shift+L</button>
          </div>
          <div class="setting-row">
            <div class="setting-copy"><strong>新建对话</strong><span>弹出窗口并开始新的会话</span></div>
            <button class="shortcut-key" id="new-chat-shortcut" data-action="newChat" type="button">Alt+Shift+N</button>
          </div>
          <div class="setting-row">
            <div class="setting-copy"><strong>查看历史会话</strong><span>弹出窗口并打开历史记录</span></div>
            <button class="shortcut-key" id="history-shortcut" data-action="history" type="button">Alt+Shift+H</button>
          </div>
          <p class="settings-error" id="shortcut-error" role="status" hidden></p>
        </section>
        <section class="settings-section">
          <h2>模型 API</h2>
          <p>密钥仅保存在本机，不会写入聊天记录，也无法从页面读取。</p>
          <p class="path-hint" id="model-config-path">读取中…</p>
          <div class="setting-field">
            <label for="model-provider">服务商</label>
            <select id="model-provider">
              <option value="deepseek">DeepSeek</option>
              <option value="custom">自定义（OpenAI 兼容）</option>
            </select>
          </div>
          <div class="setting-field">
            <label for="model-base-url">接口地址</label>
            <input id="model-base-url" type="url" spellcheck="false" autocomplete="off" placeholder="https://api.deepseek.com/v1" />
          </div>
          <div class="setting-field">
            <label for="model-name">模型</label>
            <input id="model-name" type="text" spellcheck="false" autocomplete="off" placeholder="deepseek-chat" />
          </div>
          <div class="setting-field">
            <label for="model-api-key">API Key</label>
            <div class="key-row">
              <input id="model-api-key" type="text" spellcheck="false" autocomplete="off" placeholder="sk-..." />
              <button class="plain-button" id="model-key-clear" type="button" hidden>清除</button>
            </div>
            <span class="field-hint" id="model-key-hint">尚未设置</span>
          </div>
          <label class="setting-toggle">
            <input id="model-thinking" type="checkbox" />
            <span>深度思考</span>
            <span class="field-hint">开启后模型会先推理再回答，速度较慢、消耗更多 token</span>
          </label>
          <div class="model-actions">
            <button class="plain-button preset-trigger" id="model-preset-trigger" type="button" aria-haspopup="menu" aria-expanded="false">预置模型</button>
            <button class="primary-button" id="model-save" type="button">保存</button>
            <span class="settings-error" id="model-error" role="status" hidden></span>
          </div>
        </section>
        <section class="settings-section">
          <h2>MCP 工具服务</h2>
          <p>通过 stdio 启动本机进程，模型可调用其声明的工具。修改后点「保存并重连」。</p>
          <p class="mcp-summary" id="mcp-summary">尚未配置任何工具服务</p>
          <details class="mcp-help" id="mcp-help">
            <summary>如何添加一个 MCP 服务？</summary>
            <div class="mcp-help-body">
              <p><strong>原理</strong>：MCP 是一个基于 JSON-RPC 的开放协议。LiteChat 启动你填写的命令，通过 stdin/stdout 与它握手，然后读取它声明的工具。任何实现了该协议的程序都可以接入，不限于 npm 包。</p>
              <p><strong>需要填三项</strong></p>
              <ol>
                <li><code>名称</code>：唯一标识，工具会显示为 <code>名称__工具名</code></li>
                <li><code>命令</code>：可执行文件或包管理器</li>
                <li><code>参数</code>：空格分隔</li>
              </ol>
              <p><strong>常见写法</strong></p>
              <ul class="mcp-help-examples">
                <li><code>npx</code> + <code>-y @modelcontextprotocol/server-filesystem D:\work</code><span>Node 包，官方文件系统服务</span></li>
                <li><code>uvx</code> + <code>mcp-server-fetch</code><span>Python 包，需要先装 uv</span></li>
                <li><code>D:\tools\my-mcp.exe</code> + 留空<span>本地编译好的可执行文件</span></li>
                <li><code>python</code> + <code>D:\mcp\server.py</code><span>本地 Python 脚本</span></li>
              </ul>
              <p><strong>排查</strong>：服务会随应用启动，连接失败时该行会显示具体错误。手动排查可在终端直接运行同样的命令——如果它不在等待 stdin 输入，说明不是合法的 stdio MCP 服务。</p>
              <p class="mcp-help-note">内置的 shell 工具（run_command / run_shell）不在此列表中，始终启用。</p>
            </div>
          </details>
          <div class="mcp-list" id="mcp-list"></div>
          <div class="settings-actions">
            <button class="plain-button" id="mcp-add" type="button">添加服务</button>
            <button class="plain-button" id="mcp-refresh" type="button">重新连接</button>
            <button class="primary-button" id="mcp-save" type="button">保存并重连</button>
            <span class="settings-error" id="mcp-error" role="status" hidden></span>
          </div>
        </section>
        <section class="settings-section">
          <h2>系统</h2>
          <div class="setting-row"><div class="setting-copy"><strong>系统托盘</strong><span>左键切换窗口，右键打开菜单</span></div><span class="setting-status">已启用</span></div>
        </section>
      </section>
      <section class="composer" id="composer" aria-label="消息输入区">
        <div class="attachment-list" id="attachment-list" hidden></div>
        <textarea id="message-input" rows="1" aria-label="消息" placeholder="发消息或按住空格说话..."></textarea>
        <div class="composer-toolbar">
          <button class="icon-button plus-button" id="add-attachment" type="button" aria-label="添加附件" aria-haspopup="menu" aria-expanded="false" title="添加附件">${icon("plus")}</button>
          <button class="thinking-toggle" id="thinking-toggle" type="button" aria-pressed="false" title="开启后模型会先推理再回答">深度思考</button>
          <button class="stop-button" id="stop-generation" type="button" hidden>停止生成</button>
        </div>
      </section>
      <input id="attachment-input" type="file" multiple hidden />
      <input id="folder-input" type="file" webkitdirectory directory multiple hidden />
      <input id="screenshot-input" type="file" accept="image/*" hidden />
    </main>
  `;
}
