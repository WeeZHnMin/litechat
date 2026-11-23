pub mod builtin;
pub mod commands;
pub mod files;

pub const BUILTIN_SERVER: &str = "shell";

use rmcp::model::CallToolRequestParams;
use rmcp::service::{RunningService, ServiceExt};
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::Mutex as AsyncMutex;

const CONNECT_TIMEOUT_MS: u64 = 20_000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolInfo {
    pub server: String,
    pub name: String,
    pub description: String,
    pub schema: Value,
    pub input_schema: Value,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerStatus {
    pub name: String,
    pub command: String,
    pub enabled: bool,
    pub connected: bool,
    pub tool_count: usize,
    pub error: Option<String>,
}

type Client = RunningService<rmcp::service::RoleClient, ()>;

pub struct McpManager {
    workspace: std::path::PathBuf,
    configs: Mutex<Vec<McpServerConfig>>,
    clients: AsyncMutex<HashMap<String, Client>>,
    tools: AsyncMutex<HashMap<String, Vec<McpToolInfo>>>,
    errors: Mutex<HashMap<String, String>>,
}

pub fn tool_key(server: &str, tool: &str) -> String {
    format!("{server}__{tool}")
}

fn parse_local_tools(source: Value) -> Vec<McpToolInfo> {
    source
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|tool| {
                    let name = tool.get("name")?.as_str()?.to_string();
                    let schema = tool.get("inputSchema").cloned().unwrap_or(Value::Null);
                    Some(McpToolInfo {
                        server: BUILTIN_SERVER.into(),
                        name,
                        description: tool
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        schema: schema.clone(),
                        input_schema: schema,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn server_tools(server: &str, tools: &[rmcp::model::Tool]) -> Vec<McpToolInfo> {
    tools
        .iter()
        .map(|tool| {
            let schema = Value::Object((*tool.input_schema).clone());
            McpToolInfo {
                server: server.to_string(),
                name: tool.name.to_string(),
                description: tool
                    .description
                    .as_ref()
                    .map(|text| text.to_string())
                    .unwrap_or_default(),
                schema: schema.clone(),
                input_schema: schema,
            }
        })
        .collect()
}

fn result_text(result: &rmcp::model::CallToolResult) -> String {
    let mut parts = Vec::new();
    for block in &result.content {
        if let rmcp::model::ContentBlock::Text(text) = block {
            parts.push(text.text.clone());
        }
    }
    if parts.is_empty() {
        match result.structured_content.as_ref() {
            Some(value) => value.to_string(),
            None => "（工具没有返回内容）".into(),
        }
    } else {
        parts.join("\n")
    }
}

impl McpManager {
    pub fn new(workspace: std::path::PathBuf) -> Self {
        Self {
            workspace,
            configs: Mutex::new(Vec::new()),
            clients: AsyncMutex::new(HashMap::new()),
            tools: AsyncMutex::new(HashMap::new()),
            errors: Mutex::new(HashMap::new()),
        }
    }

    pub fn workspace(&self) -> &std::path::Path {
        &self.workspace
    }

    pub fn set_configs(&self, configs: Vec<McpServerConfig>) {
        if let Ok(mut current) = self.configs.lock() {
            *current = configs;
        }
    }

    pub fn configs(&self) -> Vec<McpServerConfig> {
        self.configs
            .lock()
            .map(|value| value.clone())
            .unwrap_or_default()
    }

    pub fn record_error(&self, server: &str, error: Option<String>) {
        if let Ok(mut errors) = self.errors.lock() {
            match error {
                Some(message) => {
                    errors.insert(server.to_string(), message);
                }
                None => {
                    errors.remove(server);
                }
            }
        }
    }

    pub fn builtin_tools(&self) -> Vec<McpToolInfo> {
        let mut all = parse_local_tools(builtin::tools());
        all.extend(parse_local_tools(files::tools()));
        all
    }

    async fn connect(&self, config: &McpServerConfig) -> Result<Vec<McpToolInfo>, String> {
        let command = Command::new(&config.command).configure(|cmd| {
            cmd.args(&config.args).envs(&config.env);
        });
        let transport = TokioChildProcess::new(command)
            .map_err(|error| format!("无法启动 {}: {error}", config.command))?;
        let client = tokio::time::timeout(
            Duration::from_millis(CONNECT_TIMEOUT_MS),
            ().serve(transport),
        )
        .await
        .map_err(|_| format!("连接超时（{} 秒）", CONNECT_TIMEOUT_MS / 1000))?
        .map_err(|error| format!("握手失败: {error}"))?;
        let listed = client
            .list_tools(Default::default())
            .await
            .map_err(|error| format!("获取工具列表失败: {error}"))?;
        let tools = server_tools(&config.name, &listed.tools);
        self.clients
            .lock()
            .await
            .insert(config.name.clone(), client);
        self.tools
            .lock()
            .await
            .insert(config.name.clone(), tools.clone());
        Ok(tools)
    }

    pub async fn refresh(&self) -> Vec<McpToolInfo> {
        for client in self.clients.lock().await.drain() {
            let _ = client.1.cancel().await;
        }
        self.tools.lock().await.clear();
        self.errors.lock().ok().map(|mut map| map.clear());
        let mut all = self.builtin_tools();
        let configs: Vec<McpServerConfig> = self
            .configs()
            .into_iter()
            .filter(|config| config.enabled && config.name != BUILTIN_SERVER)
            .collect();
        for config in configs {
            match self.connect(&config).await {
                Ok(tools) => {
                    self.record_error(&config.name, None);
                    all.extend(tools);
                }
                Err(error) => self.record_error(&config.name, Some(error)),
            }
        }
        all
    }

    pub async fn tools(&self) -> Vec<McpToolInfo> {
        let configs: Vec<McpServerConfig> = self
            .configs()
            .into_iter()
            .filter(|config| config.enabled)
            .collect();
        for config in configs {
            if config.name == BUILTIN_SERVER {
                continue;
            }
            if self.tools.lock().await.contains_key(&config.name) {
                continue;
            }
            if let Err(error) = self.connect(&config).await {
                self.record_error(&config.name, Some(error));
            }
        }
        let mut all = self.builtin_tools();
        for tools in self.tools.lock().await.values() {
            all.extend(tools.clone());
        }
        all
    }

    pub fn is_builtin_tool(&self, tool: &str) -> bool {
        builtin::tool_names().contains(&tool) || files::tool_names().contains(&tool)
    }

    pub async fn call(&self, server: &str, tool: &str, arguments: Value) -> Result<String, String> {
        if server == BUILTIN_SERVER {
            return self.call_builtin(tool, arguments).await;
        }
        let clients = self.clients.lock().await;
        let client = clients
            .get(server)
            .ok_or_else(|| format!("MCP 服务 {server} 未连接"))?;
        let object = arguments
            .as_object()
            .cloned()
            .unwrap_or_default();
        let result = client
            .call_tool(CallToolRequestParams::new(tool.to_string()).with_arguments(object))
            .await
            .map_err(|error| format!("工具调用失败: {error}"))?;
        if result.is_error.unwrap_or(false) {
            return Err(result_text(&result));
        }
        Ok(result_text(&result))
    }

    pub async fn call_builtin(&self, tool: &str, arguments: Value) -> Result<String, String> {
        if tool == builtin::list_tools_tool() {
            let tools = self.tools().await;
            return Ok(builtin::describe_tools(&tools));
        }
        if files::tool_names().contains(&tool) {
            let root = self.workspace.clone();
            return files::call(&root, tool, &arguments);
        }
        builtin::call(tool, &arguments).await
    }

    pub async fn statuses(&self) -> Vec<McpServerStatus> {
        let configs = self.configs();
        let mut result = Vec::new();
        for config in configs {
            let is_builtin = config.name == BUILTIN_SERVER;
            let (connected, tool_count) = if is_builtin {
                (true, self.builtin_tools().len())
            } else {
                let clients = self.clients.lock().await;
                let connected = clients.contains_key(&config.name);
                drop(clients);
                let count = self
                    .tools
                    .lock()
                    .await
                    .get(&config.name)
                    .map(|tools| tools.len())
                    .unwrap_or(0);
                (connected, count)
            };
            let error = self
                .errors
                .lock()
                .ok()
                .and_then(|map| map.get(&config.name).cloned());
            result.push(McpServerStatus {
                name: config.name,
                command: config.command,
                enabled: config.enabled,
                connected,
                tool_count,
                error,
            });
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_keys_are_namespaced() {
        assert_eq!(tool_key("files", "read"), "files__read");
        assert_eq!(tool_key("shell", "run_command"), "shell__run_command");
    }

    #[test]
    fn builtin_tools_always_available() {
        let manager = McpManager::new(std::env::temp_dir());
        let tools = manager.builtin_tools();
        assert_eq!(tools.len(), 3 + files::tool_names().len());
        manager.set_configs(vec![]);
        assert_eq!(manager.builtin_tools().len(), tools.len());
    }

    #[test]
    fn shell_and_file_tools_both_route_to_builtin() {
        let manager = McpManager::new(std::env::temp_dir());
        assert!(manager.is_builtin_tool("run_command"));
        assert!(manager.is_builtin_tool("read_file"));
        assert!(manager.is_builtin_tool("edit_file"));
        assert!(!manager.is_builtin_tool("some_external_tool"));
    }

    #[test]
    fn flattening_prefers_text_blocks() {
        let result: rmcp::model::CallToolResult =
            serde_json::from_value(serde_json::json!({
                "content": [
                    { "type": "text", "text": "第一行" },
                    { "type": "text", "text": "第二行" }
                ]
            }))
            .expect("parse");
        assert_eq!(result_text(&result), "第一行\n第二行");
    }

    #[test]
    fn flattening_falls_back_to_structured_content() {
        let result: rmcp::model::CallToolResult =
            serde_json::from_value(serde_json::json!({
                "content": [],
                "structuredContent": { "ok": true }
            }))
            .expect("parse");
        assert_eq!(result_text(&result), r#"{"ok":true}"#);
    }

    #[test]
    fn empty_result_is_described() {
        let result: rmcp::model::CallToolResult =
            serde_json::from_value(serde_json::json!({ "content": [] })).expect("parse");
        assert_eq!(result_text(&result), "（工具没有返回内容）");
    }
}
