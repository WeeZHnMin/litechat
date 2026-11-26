use serde_json::{json, Value};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

const TIMEOUT_SECS: u64 = 30;
const MAX_OUTPUT: usize = 20_000;

const TOOLS: &str = r#"[
  {
    "name": "run_command",
    "description": "在 Windows 上执行一条 PowerShell 命令并返回输出。适合查询系统信息、列目录、查看进程等只读操作。不要执行删除、格式化、修改系统配置等破坏性操作。",
    "inputSchema": {
      "type": "object",
      "properties": {
        "command": { "type": "string", "description": "要执行的 PowerShell 命令" },
        "cwd": { "type": "string", "description": "工作目录，省略则使用默认目录" }
      },
      "required": ["command"]
    }
  },
  {
    "name": "run_shell",
    "description": "通过 cmd.exe 执行一条命令并返回输出。用于 PowerShell 无法覆盖的场景。同样避免破坏性操作。",
    "inputSchema": {
      "type": "object",
      "properties": {
        "command": { "type": "string", "description": "要执行的 cmd 命令" }
      },
      "required": ["command"]
    }
  },
  {
    "name": "list_tools",
    "description": "列出当前会话中所有可调用的工具（名称、归属服务、说明）。当你不确定有哪些能力可用，或被问到「你能做什么」「有哪些工具」时调用它，不要靠猜测或反复试探。",
    "inputSchema": {
      "type": "object",
      "properties": {},
      "required": []
    }
  },
  {
    "name": "add_mcp_server",
    "description": "为用户添加一个新的外部 MCP 工具服务。当用户说「把某个服务加到 MCP」「帮我装个 XX 工具」时使用。服务会被立即启动并验证：连接失败会自动回滚且不会保存，成功则新工具立即可用。请先用 list_tools 确认名称没有重复。",
    "inputSchema": {
      "type": "object",
      "properties": {
        "name": { "type": "string", "description": "服务的唯一标识，如 filesystem、github" },
        "command": { "type": "string", "description": "启动命令，如 npx、uvx、python、D:\\\\tools\\\\x.exe" },
        "args": { "type": "array", "items": { "type": "string" }, "description": "命令参数，如 [\"-y\", \"@modelcontextprotocol/server-filesystem\", \"D:\\\\work\"]" },
        "env": { "type": "object", "description": "额外的环境变量，通常省略" }
      },
      "required": ["name", "command"]
    }
  },
  {
    "name": "remove_mcp_server",
    "description": "移除一个外部 MCP 工具服务并停止它的进程。当用户说「把 XX 服务删掉 / 停用」「不需要这个工具了」时使用。内置的 shell 服务不能移除。",
    "inputSchema": {
      "type": "object",
      "properties": {
        "name": { "type": "string", "description": "要移除的服务名称，可先用 list_tools 查看" }
      },
      "required": ["name"]
    }
  }
]"#;

pub fn tools() -> Value {
    serde_json::from_str(TOOLS).unwrap_or_else(|_| json!([]))
}

/// PowerShell 在 stderr 被重定向时会输出 CLIXML 进度记录，过滤掉避免污染结果。
fn strip_clixml(text: &str) -> String {
    if !text.contains("<Objs Version=") {
        return text.to_string();
    }
    let mut kept = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("#< CLIXML") {
            continue;
        }
        if trimmed.contains("<Objs Version=") {
            continue;
        }
        kept.push(line);
    }
    kept.join("\n")
}

fn truncate(text: String) -> String {
    if text.chars().count() <= MAX_OUTPUT {
        return text;
    }
    let kept: String = text.chars().take(MAX_OUTPUT).collect();
    format!("{kept}\n…（输出过长，已截断）")
}

async fn run(program: &str, args: &[&str], cwd: Option<&str>) -> Result<String, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(directory) = cwd.filter(|value| !value.trim().is_empty()) {
        if !std::path::Path::new(directory).is_dir() {
            return Err(format!("工作目录不存在: {directory}"));
        }
        command.current_dir(directory);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动失败: {error}"))?;
    let stdout = child.stdout.take().ok_or("无法读取 stdout")?;
    let stderr = child.stderr.take().ok_or("无法读取 stderr")?;
    let out_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut collected = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            collected.push_str(&line);
            collected.push('\n');
        }
        collected
    });
    let err_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut collected = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            collected.push_str(&line);
            collected.push('\n');
        }
        collected
    });
    let status = tokio::time::timeout(
        std::time::Duration::from_secs(TIMEOUT_SECS),
        child.wait(),
    )
    .await
    .map_err(|_| format!("命令超时（{TIMEOUT_SECS} 秒）"))?
    .map_err(|error| format!("等待进程失败: {error}"))?;
    let stdout = out_task.await.unwrap_or_default();
    let stderr = err_task.await.unwrap_or_default();
    let stderr = strip_clixml(&stderr);
    let code = status.code().unwrap_or(-1);
    let mut output = String::new();
    if !stdout.trim().is_empty() {
        output.push_str(&stdout);
    }
    if !stderr.trim().is_empty() {
        if !output.is_empty() {
            output.push_str("\n[stderr]\n");
        }
        output.push_str(&stderr);
    }
    if output.trim().is_empty() {
        output = format!("（无输出，退出码 {code}）");
    } else if code != 0 {
        output.push_str(&format!("\n[退出码 {code}]"));
    }
    // 回显执行目录：模型据此判断相对路径解析到了哪里，避免误判文件状态
    if let Some(directory) = cwd.filter(|value| !value.trim().is_empty()) {
        output.push_str(&format!("\n[工作目录] {directory}"));
    }
    Ok(truncate(output))
}

pub fn list_tools_tool() -> &'static str {
    "list_tools"
}

pub const ADD_SERVER_TOOL: &str = "add_mcp_server";
pub const REMOVE_SERVER_TOOL: &str = "remove_mcp_server";

#[cfg(test)]
pub fn tool_names() -> Vec<&'static str> {
    vec![
        "run_command",
        "run_shell",
        list_tools_tool(),
        ADD_SERVER_TOOL,
        REMOVE_SERVER_TOOL,
    ]
}

/// PowerShell 的 -Command 参数经 Windows 命令行传递时会按系统 ANSI 代码页
/// 编码，中文和嵌套引号会损坏。改用 -EncodedCommand（UTF-16LE + base64）绕开。
fn encode_powershell(script: &str) -> String {
    use base64::Engine;
    let bytes: Vec<u8> = script
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// `workspace` 是文件工具的根目录，也是命令行工具的默认工作目录。
/// 两者必须一致，否则 `write_file` 写在 A、`run_command` 跑到 B，
/// 命令会对着不存在的路径「成功」执行，让模型误判文件状态。
pub async fn call(
    tool: &str,
    arguments: &Value,
    workspace: &std::path::Path,
) -> Result<String, String> {
    if tool == list_tools_tool() {
        return Ok(String::new());
    }
    let command = arguments
        .get("command")
        .and_then(Value::as_str)
        .ok_or("缺少 command 参数")?;
    if command.trim().is_empty() {
        return Err("command 不能为空".into());
    }
    // 调用方没指定 cwd 时回到工作目录根，与文件工具保持一致
    let default_cwd = workspace.to_string_lossy().to_string();
    let requested_cwd = arguments
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let cwd = requested_cwd.unwrap_or(&default_cwd);
    match tool {
        "run_command" => {
            let script = format!(
                "$ProgressPreference = 'SilentlyContinue'\n\
                 [Console]::OutputEncoding = [System.Text.Encoding]::UTF8\n\
                 $OutputEncoding = [System.Text.Encoding]::UTF8\n\
                 {command}"
            );
            let encoded = encode_powershell(&script);
            run(
                "powershell",
                &["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded],
                Some(cwd),
            )
            .await
        }
        "run_shell" => run("cmd", &["/C", command], Some(cwd)).await,
        other => Err(format!("未知工具: {other}")),
    }
}

pub fn describe_tools(tools: &[crate::tools::mcp::McpToolInfo]) -> String {
    if tools.is_empty() {
        return "当前没有可调用的工具。".into();
    }
    let mut lines = vec![format!("当前共 {} 个可用工具：", tools.len())];
    for tool in tools {
        let description = if tool.description.trim().is_empty() {
            "（无说明）".to_string()
        } else {
            tool.description.replace('\n', " ")
        };
        lines.push(format!(
            "- {}__{} [{}] {}",
            tool.server, tool.name, tool.server, description
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_command_is_utf16le_base64() {
        use base64::Engine;
        let encoded = encode_powershell("hi");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded.as_bytes())
            .expect("合法 base64");
        assert_eq!(bytes, vec![b'h', 0, b'i', 0]);
        let decoded = String::from_utf16(
            &bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(decoded, "hi");
    }

    #[test]
    fn encoded_command_preserves_chinese_and_quotes() {
        use base64::Engine;
        let script = "Write-Output \"中文 $(Get-Location)\"";
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encode_powershell(script).as_bytes())
            .unwrap();
        let decoded = String::from_utf16(
            &bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(decoded, script);
    }

    #[test]
    fn clixml_noise_is_stripped() {
        let noisy = "#< CLIXML\n<Objs Version=\"1.1.0.1\" xmlns=\"http://schemas.microsoft.com/powershell/2004/04\"><Obj S=\"progress\"><AV>正在准备首次使用模块。</AV></Obj></Objs>\n真正的内容";
        assert_eq!(strip_clixml(noisy), "真正的内容");
    }

    #[test]
    fn plain_stderr_passes_through() {
        assert_eq!(strip_clixml("error: something failed"), "error: something failed");
    }

    #[test]
    fn exposes_expected_tools() {
        let list = tools();
        let array = list.as_array().expect("array");
        let names: Vec<&str> = array.iter().filter_map(|t| t["name"].as_str()).collect();
        assert_eq!(
            names,
            vec![
                "run_command",
                "run_shell",
                "list_tools",
                "add_mcp_server",
                "remove_mcp_server"
            ]
        );
        assert_eq!(array[0]["inputSchema"]["required"][0], "command");
    }

    #[test]
    fn add_and_remove_tools_require_name() {
        let add = tools()
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "add_mcp_server")
            .cloned()
            .unwrap();
        let required: Vec<&str> = add["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(required.contains(&"name"));
        assert!(required.contains(&"command"));
    }

    #[test]
    fn describe_tools_lists_names_and_servers() {
        let tools = vec![
            crate::tools::mcp::McpToolInfo {
                server: "shell".into(),
                name: "run_command".into(),
                description: "执行命令".into(),
                schema: json!({}),
                input_schema: json!({}),
            },
            crate::tools::mcp::McpToolInfo {
                server: "files".into(),
                name: "read_file".into(),
                description: String::new(),
                schema: json!({}),
                input_schema: json!({}),
            },
        ];
        let text = describe_tools(&tools);
        assert!(text.contains("共 2 个"));
        assert!(text.contains("shell__run_command"));
        assert!(text.contains("files__read_file"));
        assert!(text.contains("（无说明）"));
    }

    #[test]
    fn describe_tools_handles_empty() {
        assert_eq!(describe_tools(&[]), "当前没有可调用的工具。");
    }

    fn workspace() -> std::path::PathBuf {
        std::env::temp_dir()
    }

    /// 读 stdout/stderr 的任务要和 wait() 并发，必须是多线程运行时。
    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime")
    }

    #[test]
    fn rejects_missing_command() {
        let runtime = runtime();
        let result = runtime.block_on(call("run_command", &json!({}), &workspace()));
        assert!(result.is_err());
    }

    #[test]
    fn rejects_empty_command() {
        let runtime = runtime();
        let result = runtime.block_on(call("run_shell", &json!({ "command": "  " }), &workspace()));
        assert!(result.is_err());
    }

    /// Windows 路径比较要忽略 `\\?\` 前缀和尾部反斜杠。
    fn normalize(path: &str) -> String {
        path.trim()
            .trim_start_matches(r"\\?\")
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_ascii_lowercase()
    }

    #[cfg(windows)]
    #[test]
    fn commands_default_to_the_workspace_root() {
        let runtime = runtime();
        let root = std::env::temp_dir();
        // 不传 cwd 时应当跑在工作目录里，而不是进程自身目录
        let output = runtime
            .block_on(call(
                "run_command",
                &json!({ "command": "Write-Output \"PWD=$((Get-Location).Path)\"" }),
                &root,
            ))
            .expect("命令应能执行");
        let line = output
            .lines()
            .find(|line| line.starts_with("PWD="))
            .unwrap_or_else(|| panic!("没有输出 PWD：{output}"));
        let actual = normalize(&line[4..]);
        let expected = normalize(&root.display().to_string());
        assert_eq!(actual, expected, "命令应默认在工作目录执行：{output}");
    }

    #[cfg(windows)]
    #[test]
    fn explicit_cwd_still_wins() {
        let runtime = runtime();
        let elsewhere = std::env::temp_dir();
        let output = runtime
            .block_on(call(
                "run_command",
                &json!({
                    "command": "Write-Output \"PWD=$((Get-Location).Path)\"",
                    "cwd": elsewhere.display().to_string()
                }),
                &std::path::Path::new("C:\\"),
            ))
            .expect("命令应能执行");
        let line = output
            .lines()
            .find(|line| line.starts_with("PWD="))
            .unwrap_or_else(|| panic!("没有输出 PWD：{output}"));
        let actual = normalize(&line[4..]);
        let expected = normalize(&elsewhere.display().to_string());
        assert_eq!(actual, expected, "显式 cwd 应优先：{output}");
    }
}
