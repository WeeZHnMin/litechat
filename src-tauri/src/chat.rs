use crate::mcp::{self, McpManager};
use crate::models;
use crate::providers::{self, ChatMessage, ProviderConfig, StreamEvent, ToolCall};
use crate::storage::{Storage, StoredMessage as StorageMessage};
use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

const MAX_TOOL_ROUNDS: usize = 6;

#[derive(Default)]
pub struct ActiveStreams(Mutex<Vec<String>>);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeltaPayload {
    stream_id: String,
    text: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReasoningPayload {
    stream_id: String,
    text: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolCallPayload {
    stream_id: String,
    id: String,
    name: String,
    server: String,
    arguments: String,
    phase: String,
    result: Option<String>,
    error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DonePayload {
    stream_id: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorPayload {
    stream_id: String,
    message: String,
}

const SYSTEM_PROMPT: &str = "你是一个乐于助人的助手。回答简洁准确，使用 Markdown 格式。需要外部信息或执行操作时，调用提供的工具，并根据工具结果继续回答。";

pub fn message_text(parts: &serde_json::Value) -> String {
    let Some(items) = parts.as_array() else {
        return String::new();
    };
    items
        .iter()
        .filter(|part| {
            matches!(
                part.get("type").and_then(|value| value.as_str()),
                Some("text") | Some("markdown")
            )
        })
        .filter_map(|part| part.get("text").and_then(|value| value.as_str()))
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim()
        .to_string()
}

fn build_messages(stored: Vec<StorageMessage>) -> Vec<ChatMessage> {
    let mut messages = vec![ChatMessage::text("system", SYSTEM_PROMPT)];
    for item in stored {
        let text = message_text(&item.parts);
        if text.is_empty() {
            continue;
        }
        messages.push(ChatMessage::text(item.role, text));
    }
    messages
}

fn store_outgoing(app: &AppHandle, conversation_id: &str, user_text: &str) -> Result<(), String> {
    if user_text.trim().is_empty() {
        return Ok(());
    }
    let storage = app.state::<Storage>();
    storage.insert_message(
        conversation_id.to_string(),
        "user".into(),
        serde_json::json!([{ "type": "text", "text": user_text }]),
    )?;
    Ok(())
}

#[tauri::command]
pub async fn send_chat(
    app: AppHandle,
    conversation_id: String,
    text: String,
) -> Result<String, String> {
    let config = models::snapshot(&app)?;
    let provider = ProviderConfig {
        base_url: config.base_url,
        model: config.model,
        api_key: config.api_key,
        thinking: config.thinking,
    };
    store_outgoing(&app, &conversation_id, &text)?;
    let stored = {
        let storage = app.state::<Storage>();
        storage.messages(&conversation_id)?
    };
    let messages = build_messages(stored);
    let stream_id = uuid::Uuid::new_v4().to_string();
    {
        let state = app.state::<ActiveStreams>();
        let mut active = state.0.lock().map_err(|error| error.to_string())?;
        active.push(stream_id.clone());
    }
    let handle = app.clone();
    let id = stream_id.clone();
    tauri::async_runtime::spawn(async move {
        let result = run_turn(handle.clone(), provider, messages, id.clone()).await;
        {
            let state = handle.state::<ActiveStreams>();
            if let Ok(mut active) = state.0.lock() {
                active.retain(|item| item != &id);
            };
        }
        match result {
            Ok(()) => {
                let _ = handle.emit("chat://done", DonePayload { stream_id: id });
            }
            Err(message) => {
                let _ = handle.emit(
                    "chat://error",
                    ErrorPayload {
                        stream_id: id,
                        message,
                    },
                );
            }
        }
    });
    Ok(stream_id)
}

async fn run_turn(
    app: AppHandle,
    provider: ProviderConfig,
    mut messages: Vec<ChatMessage>,
    stream_id: String,
) -> Result<(), String> {
    let tools = app.state::<McpManager>().tools().await;
    let specs: Vec<providers::ToolSpec> = tools
        .iter()
        .map(|tool| {
            providers::tool_spec(
                mcp::tool_key(&tool.server, &tool.name),
                tool.description.clone(),
                tool.input_schema.clone(),
            )
        })
        .collect();
    let registry: std::collections::HashMap<String, (String, String)> = tools
        .iter()
        .map(|tool| {
            (
                mcp::tool_key(&tool.server, &tool.name),
                (tool.server.clone(), tool.name.clone()),
            )
        })
        .collect();

    let mut seen: Vec<String> = Vec::new();
    for _ in 0..MAX_TOOL_ROUNDS {
        if !is_active(&app, &stream_id) {
            return Ok(());
        }
        let calls = stream_once(&app, &provider, &messages, &specs, &stream_id).await?;
        if calls.is_empty() {
            return Ok(());
        }
        let signature: String = calls
            .iter()
            .map(|call| format!("{}({})", call.function.name, call.function.arguments))
            .collect::<Vec<_>>()
            .join("|");
        if seen.contains(&signature) {
            messages.push(ChatMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(calls.clone()),
                tool_call_id: None,
            });
            for call in &calls {
                messages.push(ChatMessage::tool_result(
                    call.id.clone(),
                    "同一调用已执行过，结果见上文。请直接使用已有结果回答，不要再重复调用。".into(),
                ));
            }
            break;
        }
        seen.push(signature);
        messages.push(ChatMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(calls.clone()),
            tool_call_id: None,
        });
        for call in calls {
            let Some((server, tool)) = registry.get(&call.function.name) else {
                messages.push(ChatMessage::tool_result(
                    call.id.clone(),
                    format!("未知工具：{}", call.function.name),
                ));
                continue;
            };
            let arguments: serde_json::Value = serde_json::from_str(&call.function.arguments)
                .unwrap_or_else(|_| serde_json::json!({}));
            let _ = app.emit(
                "chat://tool-call",
                ToolCallPayload {
                    stream_id: stream_id.clone(),
                    id: call.id.clone(),
                    name: tool.clone(),
                    server: server.clone(),
                    arguments: call.function.arguments.clone(),
                    phase: "running".into(),
                    result: None,
                    error: None,
                },
            );
            let outcome = app
                .state::<McpManager>()
                .call(&app, server, tool, arguments)
                .await;
            let (output, error) = match outcome {
                Ok(text) => (text, None),
                Err(message) => (format!("工具执行失败：{message}"), Some(message)),
            };
            let _ = app.emit(
                "chat://tool-call",
                ToolCallPayload {
                    stream_id: stream_id.clone(),
                    id: call.id.clone(),
                    name: tool.clone(),
                    server: server.clone(),
                    arguments: call.function.arguments.clone(),
                    phase: "done".into(),
                    result: Some(truncate(&output, 4000)),
                    error,
                },
            );
            messages.push(ChatMessage::tool_result(call.id.clone(), truncate(&output, 20000)));
        }
    }
    let last = stream_once(&app, &provider, &messages, &specs, &stream_id).await?;
    if last.is_empty() {
        return Ok(());
    }
    Err("工具调用轮次过多，已停止".into())
}

pub fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let kept: String = text.chars().take(limit).collect();
    format!("{kept}…（已截断）")
}

async fn stream_once(
    app: &AppHandle,
    provider: &ProviderConfig,
    messages: &[ChatMessage],
    specs: &[providers::ToolSpec],
    stream_id: &str,
) -> Result<Vec<ToolCall>, String> {
    let emit_handle = app.clone();
    let delta_id = stream_id.to_string();
    let call_holder = std::sync::Arc::new(Mutex::new(Vec::new()));
    let collector = call_holder.clone();
    providers::stream_chat(
        provider,
        messages.to_vec(),
        specs.to_vec(),
        move |event| {
            if !is_active(&emit_handle, &delta_id) {
                return false;
            }
            match event {
                StreamEvent::Delta(text) => {
                    let _ = emit_handle.emit(
                        "chat://delta",
                        DeltaPayload {
                            stream_id: delta_id.clone(),
                            text,
                        },
                    );
                }
                StreamEvent::Reasoning(text) => {
                    let _ = emit_handle.emit(
                        "chat://reasoning",
                        ReasoningPayload {
                            stream_id: delta_id.clone(),
                            text,
                        },
                    );
                }
                StreamEvent::ToolCalls(calls) => {
                    if let Ok(mut guard) = collector.lock() {
                        guard.extend(calls);
                    }
                }
                StreamEvent::Done => {}
            }
            true
        },
    )
    .await
    .map_err(|error| error.message())?;
    let calls = call_holder
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    Ok(calls)
}

fn is_active(app: &AppHandle, stream_id: &str) -> bool {
    app.state::<ActiveStreams>()
        .0
        .lock()
        .map(|active| active.iter().any(|item| item == stream_id))
        .unwrap_or(false)
}

#[tauri::command]
pub fn cancel_stream(app: AppHandle, stream_id: String) -> Result<(), String> {
    let state = app.state::<ActiveStreams>();
    let mut active = state.0.lock().map_err(|error| error.to_string())?;
    active.retain(|item| item != &stream_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payloads_serialize_with_camel_case_keys() {
        let delta = serde_json::to_value(DeltaPayload {
            stream_id: "abc".into(),
            text: "hi".into(),
        })
        .unwrap();
        assert_eq!(delta["streamId"], "abc");
        assert!(delta.get("stream_id").is_none());

        let done = serde_json::to_value(DonePayload {
            stream_id: "abc".into(),
        })
        .unwrap();
        assert_eq!(done["streamId"], "abc");

        let error = serde_json::to_value(ErrorPayload {
            stream_id: "abc".into(),
            message: "boom".into(),
        })
        .unwrap();
        assert_eq!(error["streamId"], "abc");
        assert_eq!(error["message"], "boom");
    }

    #[test]
    fn projection_keeps_only_text_parts() {
        let parts = serde_json::json!([
            { "type": "markdown", "text": "第一段" },
            { "type": "text", "text": "第二段" },
            { "type": "image", "url": "blob:http://localhost/x", "name": "a.png" },
            { "type": "file", "url": "blob:http://localhost/y", "name": "b.pdf", "size": 10 }
        ]);
        assert_eq!(message_text(&parts), "第一段\n\n第二段");
    }

    #[test]
    fn projection_drops_messages_without_text() {
        let parts = serde_json::json!([
            { "type": "image", "url": "blob:http://localhost/x" }
        ]);
        assert_eq!(message_text(&parts), "");
    }

    #[test]
    fn projection_excludes_reasoning_content() {
        let parts = serde_json::json!([
            { "type": "reasoning", "text": "我先想想" },
            { "type": "markdown", "text": "答案" }
        ]);
        assert_eq!(message_text(&parts), "答案");
    }

    #[test]
    fn projection_of_reasoning_only_message_is_empty() {
        let parts = serde_json::json!([
            { "type": "reasoning", "text": "只有思考" }
        ]);
        assert_eq!(message_text(&parts), "");
    }

    #[test]
    fn history_is_prefixed_with_system_prompt() {
        let stored = vec![
            StorageMessage {
                id: "1".into(),
                conversation_id: "c".into(),
                role: "user".into(),
                parts: serde_json::json!([{ "type": "text", "text": "你好" }]),
                created_at: 1,
            },
            StorageMessage {
                id: "2".into(),
                conversation_id: "c".into(),
                role: "assistant".into(),
                parts: serde_json::json!([{ "type": "markdown", "text": "在" }]),
                created_at: 2,
            },
        ];
        let messages = build_messages(stored);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[1].content.as_deref(), Some("你好"));
        assert_eq!(messages[2].role, "assistant");
        assert_eq!(messages[2].content.as_deref(), Some("在"));
    }
}
