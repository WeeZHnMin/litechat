import { icon } from "../app/icons";

const shortcutRows = [
  ["toggle", "显示 / 隐藏窗口", "弹出、聚焦或隐藏到系统托盘", "Alt+Shift+L"],
  ["newChat", "新建对话", "弹出窗口并开始新的会话", "Alt+Shift+N"],
  ["history", "查看历史会话", "弹出窗口并打开历史记录", "Alt+Shift+H"],
]
  .map(
    ([action, title, hint, accelerator]) => `
      <div class="setting-row">
        <div class="setting-copy">
          <strong>${title}</strong>
          <span>${hint}</span>
        </div>
        <button class="shortcut-key" id="${action}-shortcut" data-action="${action}" type="button">${accelerator}</button>
      </div>`,
  )
  .join("");

const presets = [
  ["filesystem", "文件读写（官方）", "需要 Node，在设置里选目录"],
  ["fetch", "网页抓取（官方）", "需要 uv，读取 URL 内容"],
  ["memory", "记忆存储（官方）", "需要 Node，跨对话记住信息"],
  ["custom", "自定义 / 我自己写", "看到下面的说明"],
]
  .map(
    ([key, title, hint]) => `
      <button class="mcp-preset" type="button" data-preset="${key}">
        <strong>${title}</strong>
        <span>${hint}</span>
      </button>`,
  )
  .join("");

export function settingsViewHtml() {
  return `
    <section class="settings-view" id="settings-view" aria-label="设置" tabindex="-1" hidden>
      <div class="settings-page">
      <header class="settings-header">
        <button class="icon-button" id="settings-back" type="button" aria-label="返回聊天" title="返回聊天">${icon("back")}</button>
        <h1>设置</h1>
      </header>

      <section class="settings-section">
        <div class="settings-section-head">
          <h2>全局快捷键</h2>
          <p>LiteChat 在后台或托盘时同样有效。</p>
        </div>
        <div class="settings-card">${shortcutRows}</div>
        <p class="settings-error" id="shortcut-error" role="status" hidden></p>
      </section>

      <section class="settings-section">
        <div class="settings-section-head">
          <h2>模型 API</h2>
          <p>密钥只保存在本机，不写入聊天记录，页面也读不到明文。</p>
        </div>
        <div class="settings-card settings-card--padded">
          <div class="settings-field-grid">
            <div class="setting-field">
              <label for="model-provider">服务商</label>
              <select id="model-provider">
                <option value="deepseek">DeepSeek</option>
                <option value="custom">自定义（OpenAI 兼容）</option>
              </select>
            </div>
            <div class="setting-field">
              <label for="model-name">模型</label>
              <div class="key-row">
                <input id="model-name" type="text" spellcheck="false" autocomplete="off" placeholder="deepseek-chat" />
                <button class="plain-button test-button" id="model-scan" type="button" aria-haspopup="menu" aria-expanded="false">
                  <span class="button-label">获取列表</span>
                  <span class="button-spinner" aria-hidden="true"></span>
                </button>
              </div>
            </div>
          </div>
          <div class="setting-field">
            <label for="model-base-url">接口地址</label>
            <input id="model-base-url" type="url" spellcheck="false" autocomplete="off" placeholder="https://api.deepseek.com/v1" />
          </div>
          <div class="setting-field">
            <label for="model-api-key">API Key</label>
            <div class="key-row">
              <input id="model-api-key" type="text" spellcheck="false" autocomplete="off" placeholder="sk-..." />
              <button class="plain-button" id="model-key-clear" type="button" hidden>清除</button>
            </div>
            <span class="field-hint" id="model-key-hint">尚未设置</span>
          </div>
          <div class="model-actions">
            <button class="plain-button preset-trigger" id="model-preset-trigger" type="button" aria-haspopup="menu" aria-expanded="false">预置模型</button>
            <button class="plain-button test-button" id="model-test" type="button">
              <span class="button-label">测试连接</span>
              <span class="button-spinner" aria-hidden="true"></span>
            </button>
            <button class="primary-button" id="model-save" type="button">保存</button>
          </div>
          <p class="settings-error" id="model-error" role="status" hidden></p>
          <p class="settings-note" id="model-test-result" role="status" hidden></p>
          <p class="path-hint" id="model-config-path">读取中…</p>
        </div>
      </section>

      <section class="settings-section">
        <div class="settings-section-head">
          <h2>文件工具</h2>
          <p>内置的文件读写工具只能访问这个目录及其子目录。上传的附件也会存在这里的 uploads/ 下。</p>
        </div>
        <div class="settings-card settings-card--padded">
          <div class="setting-field">
            <label for="workspace-path">工作目录</label>
            <div class="key-row">
              <input id="workspace-path" type="text" spellcheck="false" autocomplete="off" placeholder="选择目录" />
              <button class="plain-button" id="workspace-save" type="button">保存</button>
            </div>
            <span class="field-hint" id="workspace-hint">所有相对路径都基于此目录</span>
          </div>
        </div>
      </section>

      <section class="settings-section">
        <div class="settings-section-head">
          <h2>MCP 工具服务</h2>
          <p>通过 stdio 启动本机进程，模型可调用其声明的工具。</p>
        </div>
        <div class="settings-card settings-card--padded">
          <p class="mcp-summary" id="mcp-summary">尚未配置任何工具服务</p>
          <div class="mcp-list" id="mcp-list"></div>
          <div class="mcp-presets" id="mcp-presets">
            <p class="mcp-presets-title">不想手填？点一个直接加进来：</p>
            <div class="mcp-preset-grid">${presets}</div>
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
        </div>
      </section>

      <section class="settings-section">
        <div class="settings-section-head">
          <h2>系统</h2>
          <p>关闭窗口时保持后台运行。</p>
        </div>
        <div class="settings-card">
          <div class="setting-row">
            <div class="setting-copy">
              <strong>系统托盘</strong>
              <span>左键切换窗口，右键打开菜单</span>
            </div>
            <span class="setting-status setting-status--on">已启用</span>
          </div>
        </div>
      </section>
      </div>
    </section>
  `;
}
