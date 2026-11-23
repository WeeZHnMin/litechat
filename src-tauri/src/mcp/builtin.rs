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
  }
]"#;

pub fn tools() -> Value {
    serde_json::from_str(TOOLS).unwrap_or_else(|_| json!([]))
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
    Ok(truncate(output))
}

pub fn list_tools_tool() -> &'static str {
    "list_tools"
}

pub fn tool_names() -> Vec<&'static str> {
    vec!["run_command", "run_shell", list_tools_tool()]
}

pub async fn call(tool: &str, arguments: &Value) -> Result<String, String> {
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
    match tool {
        "run_command" => {
            let cwd = arguments.get("cwd").and_then(Value::as_str);
            run(
                "powershell",
                &["-NoProfile", "-NonInteractive", "-Command", command],
                cwd,
            )
            .await
        }
        "run_shell" => run("cmd", &["/C", command], None).await,
        other => Err(format!("未知工具: {other}")),
    }
}

pub fn describe_tools(tools: &[crate::mcp::McpToolInfo]) -> String {
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
    fn exposes_expected_tools() {
        let list = tools();
        let array = list.as_array().expect("array");
        assert_eq!(array.len(), 3);
        assert_eq!(array[0]["name"], "run_command");
        assert_eq!(array[1]["name"], "run_shell");
        assert_eq!(array[2]["name"], "list_tools");
        assert_eq!(array[0]["inputSchema"]["required"][0], "command");
    }

    #[test]
    fn describe_tools_lists_names_and_servers() {
        let tools = vec![
            crate::mcp::McpToolInfo {
                server: "shell".into(),
                name: "run_command".into(),
                description: "执行命令".into(),
                schema: json!({}),
                input_schema: json!({}),
            },
            crate::mcp::McpToolInfo {
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

    #[test]
    fn rejects_missing_command() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        let result = runtime.block_on(call("run_command", &json!({})));
        assert!(result.is_err());
    }

    #[test]
    fn rejects_empty_command() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        let result = runtime.block_on(call("run_shell", &json!({ "command": "  " })));
        assert!(result.is_err());
    }
}
