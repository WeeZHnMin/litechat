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
          <h2>文件工具</h2>
          <p>内置的文件读写工具只能访问这个目录及其子目录。</p>
          <div class="setting-field">
            <label for="workspace-path">工作目录</label>
            <div class="key-row">
              <input id="workspace-path" type="text" spellcheck="false" autocomplete="off" placeholder="选择目录" />
              <button class="plain-button" id="workspace-save" type="button">保存</button>
            </div>
            <span class="field-hint" id="workspace-hint">所有相对路径都基于此目录</span>
          </div>
        </section>
        <section class="settings-section">
          <h2>MCP 工具服务</h2>
          <p>通过 stdio 启动本机进程，模型可调用其声明的工具。修改后点「保存并重连」。</p>
          <p class="mcp-summary" id="mcp-summary">尚未配置任何工具服务</p>
          <div class="mcp-list" id="mcp-list"></div>
          <div class="mcp-presets" id="mcp-presets">
            <p class="mcp-presets-title">不想手填？点一个直接加进来：</p>
            <div class="mcp-preset-grid">
              <button class="mcp-preset" type="button" data-preset="filesystem">
                <strong>文件读写（官方）</strong>
                <span>需要 Node，在设置里选目录</span>
              </button>
              <button class="mcp-preset" type="button" data-preset="fetch">
                <strong>网页抓取（官方）</strong>
                <span>需要 uv，读取 URL 内容</span>
              </button>
              <button class="mcp-preset" type="button" data-preset="memory">
                <strong>记忆存储（官方）</strong>
                <span>需要 Node，跨对话记住信息</span>
              </button>
              <button class="mcp-preset" type="button" data-preset="custom">
                <strong>自定义 / 我自己写</strong>
                <span>看到下面的说明</span>
              </button>
            </div>
          </div>
          <details class="mcp-help" id="mcp-help">
            <summary>我要接自己写的程序，怎么填？</summary>
            <div class="mcp-help-body">
              <div class="mcp-step">
                <span class="mcp-step-num">1</span>
                <div>
                  <p><strong>先理解一件事</strong></p>
                  <p>MCP 服务不是「普通的命令行程序」。普通程序跑完就退出；MCP 服务要一直挂着，通过标准输入输出跟 LiteChat 收发消息。</p>
                  <p class="mcp-compare">
                    <code>my_exam.exe -upload a.txt</code> <span>← 普通程序，跑完退出，不能直接填</span><br />
                    <code>my_exam-mcp.exe</code> <span>← MCP 服务，一直等待指令，可以填</span>
                  </p>
                </div>
              </div>
              <div class="mcp-step">
                <span class="mcp-step-num">2</span>
                <div>
                  <p><strong>给普通程序套一层外壳</strong></p>
                  <p>写一个几十行的脚本，把 MCP 协议翻译成你的命令行参数。下面是个可运行的完整模板：</p>
                  <pre class="mcp-code">import json, subprocess, sys

TOOLS = [{
    "name": "exam_upload",
    "description": "上传考试结果到服务器",
    "inputSchema": {
        "type": "object",
        "properties": {
            "file": {"type": "string", "description": "本地文件路径"},
            "force": {"type": "boolean", "description": "强制覆盖"}
        },
        "required": ["file"]
    }
}]

def handle(req):
    method = req.get("method")
    if method == "initialize":
        return {"protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "exam", "version": "1.0"}}
    if method == "tools/list":
        return {"tools": TOOLS}
    if method == "tools/call":
        args = req["params"]["arguments"]
        cmd = ["my_exam.exe", "-upload", args["file"]]
        if args.get("force"):
            cmd.append("--force")
        out = subprocess.run(cmd, capture_output=True, text=True)
        return {"content": [{"type": "text",
                             "text": out.stdout or out.stderr}]}
    return {}

for line in sys.stdin:
    req = json.loads(line)
    if "id" not in req:
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": req["id"],
                      "result": handle(req)}), flush=True)</pre>
                  <p>保存为 <code>D:\mcp\exam.py</code>，把 <code>cmd = [...]</code> 那行改成你真实的命令。</p>
                </div>
              </div>
              <div class="mcp-step">
                <span class="mcp-step-num">3</span>
                <div>
                  <p><strong>填进下面的表单</strong></p>
                  <p>
                    <code>名称</code>：<code>exam</code>（随便起，工具会显示成 <code>exam__exam_upload</code>）<br />
                    <code>命令</code>：<code>python</code><br />
                    <code>参数</code>：<code>D:\mcp\exam.py</code>
                  </p>
                  <p>然后点「保存并重连」。连上后状态会显示工具数量，模型就能用了。</p>
                </div>
              </div>
              <div class="mcp-step">
                <span class="mcp-step-num">4</span>
                <div>
                  <p><strong>连不上怎么办</strong></p>
                  <p>在终端里直接运行同样的命令：</p>
                  <p><code>python D:\mcp\exam.py</code></p>
                  <p>如果它立刻退出或报错，说明脚本本身有问题；如果它停在那里等你输入，说明正常。此时可以粘贴下面这行测试：</p>
                  <p><code>{"jsonrpc":"2.0","id":1,"method":"tools/list"}</code></p>
                  <p>应该回一行含工具列表的 JSON。</p>
                </div>
              </div>
            </div>
          </details>
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
          <button class="thinking-toggle permission-toggle" id="permission-toggle" type="button" aria-pressed="false" aria-haspopup="menu" aria-expanded="false" title="工具执行权限">每次询问</button>
          <button class="stop-button" id="stop-generation" type="button" hidden>停止生成</button>
        </div>
      </section>
      <input id="attachment-input" type="file" multiple hidden />
      <input id="folder-input" type="file" webkitdirectory directory multiple hidden />
      <input id="screenshot-input" type="file" accept="image/*" hidden />
    </main>
  `;
}
