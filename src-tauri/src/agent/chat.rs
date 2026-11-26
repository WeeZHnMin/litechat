use crate::tools::mcp::{self, McpManager};
use crate::agent::models;
use crate::agent::providers::{self, ChatMessage, ProviderConfig, StreamEvent, ToolCall};
use crate::app::storage::Storage;
use super::projection::build_messages;
use super::text::truncate;
use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

const MAX_TOOL_ROUNDS: usize = 200;
const MAX_TOOL_CALLS: usize = 500;

#[derive(Default)]
pub struct ActiveStreams(Mutex<Vec<String>>);

/// 生成过程中用户新发的消息。按 stream_id 分组，等下一个模型请求开始时注入。
#[derive(Default)]
pub struct QueuedInput(Mutex<std::collections::HashMap<String, Vec<String>>>);

impl QueuedInput {
    fn push(&self, stream_id: &str, text: String) {
        if let Ok(mut map) = self.0.lock() {
            map.entry(stream_id.to_string()).or_default().push(text);
        }
    }

    fn take(&self, stream_id: &str) -> Vec<String> {
        self.0
            .lock()
            .ok()
            .and_then(|mut map| map.remove(stream_id))
            .unwrap_or_default()
    }

    fn drop_stream(&self, stream_id: &str) {
        if let Ok(mut map) = self.0.lock() {
            map.remove(stream_id);
        }
    }
}

#[tauri::command]
pub fn queue_user_input(
    app: AppHandle,
    stream_id: String,
    text: String,
) -> Result<(), String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("不能发送空消息".into());
    }
    if !is_active(&app, &stream_id) {
        return Err("这条回复已经结束".into());
    }
    app.state::<QueuedInput>()
        .push(&stream_id, trimmed.to_string());
    Ok(())
}

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
struct InputAcceptedPayload {
    stream_id: String,
    text: String,
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

#[tauri::command]
pub async fn send_chat(
    app: AppHandle,
    conversation_id: String,
) -> Result<String, String> {
    let config = models::snapshot(&app)?;
    let provider = ProviderConfig {
        base_url: config.base_url,
        model: config.model,
        api_key: config.api_key,
        reasoning_effort: config.reasoning_effort,
    };
    let stored = {
        let storage = app.state::<Storage>();
        storage.messages(&conversation_id)?
    };
    if stored.is_empty() {
        return Err("会话里没有消息".into());
    }
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
        handle.state::<QueuedInput>().drop_stream(&id);
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

    app.state::<crate::agent::permissions::Permissions>().begin_turn();
    let mut seen: Vec<String> = Vec::new();
    let mut executed = 0usize;
    for _ in 0..MAX_TOOL_ROUNDS {
        if !is_active(&app, &stream_id) {
            return Ok(());
        }
        // 上一段输出已结束、工具还没跑（或刚跑完）——此时插入用户新消息，
        // 让模型在下一段思考开始前就看见它，顺序天然正确。
        drain_queued_input(&app, &stream_id, &mut messages);
        let calls = stream_once(&app, &provider, &messages, &specs, &stream_id).await?;
        if calls.is_empty() {
            // 模型已给出最终答复。但用户可能是在这一轮进行中才发的消息，
            // 那样它一直没被读到，补一轮让模型回应。
            answer_late_input(&app, &provider, &mut messages, &stream_id).await?;
            return Ok(());
        }
        if executed + calls.len() > MAX_TOOL_CALLS {
            return wrap_up(&app, &provider, &messages, &stream_id).await;
        }
        executed += calls.len();
        let signature: String = calls
            .iter()
            .map(|call| format!("{}({})", call.function.name, call.function.arguments))
            .collect::<Vec<_>>()
            .join("|");
        if seen.contains(&signature) {
            let repeats = repeats_of(&seen, &signature);
            messages.push(ChatMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(calls.clone()),
                tool_call_id: None,
            });
            for call in &calls {
                messages.push(ChatMessage::tool_result(
                    call.id.clone(),
                    "这次调用与之前完全相同，结果见上文。请直接使用已有结果，不要再重复调用。".into(),
                ));
            }
            if repeats >= 2 {
                return wrap_up(&app, &provider, &messages, &stream_id).await;
            }
            seen.push(signature);
            continue;
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
            let outcome = match crate::agent::permissions::ensure_allowed(
                &app,
                tool,
                server,
                &call.function.arguments,
            )
            .await
            {
                Ok(()) => {
                    app.state::<McpManager>()
                        .call(&app, server, tool, arguments)
                        .await
                }
                Err(message) => Err(message),
            };
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
    wrap_up(&app, &provider, &messages, &stream_id).await
}

pub fn repeats_of(seen: &[String], signature: &str) -> usize {
    seen.iter().filter(|item| item.as_str() == signature).count()
}

/// 把用户在这一轮里插入的消息取出来，按顺序追加为用户消息。
/// 返回是否真的取到了内容。
fn drain_queued_input(
    app: &AppHandle,
    stream_id: &str,
    messages: &mut Vec<ChatMessage>,
) -> bool {
    let queued = app.state::<QueuedInput>().take(stream_id);
    if queued.is_empty() {
        return false;
    }
    for text in queued {
        let _ = app.emit(
            "chat://input-accepted",
            InputAcceptedPayload {
                stream_id: stream_id.to_string(),
                text: text.clone(),
            },
        );
        messages.push(ChatMessage::text("user", text));
    }
    true
}

/// 模型已经写完最终答复后才到达的用户消息：再请求一次让它回应。
/// 最多循环几轮，避免用户连续输入时无限延长。
async fn answer_late_input(
    app: &AppHandle,
    provider: &ProviderConfig,
    messages: &mut Vec<ChatMessage>,
    stream_id: &str,
) -> Result<(), String> {
    const MAX_LATE_ROUNDS: usize = 3;
    for _ in 0..MAX_LATE_ROUNDS {
        if !is_active(app, stream_id) {
            return Ok(());
        }
        if !drain_queued_input(app, stream_id, messages) {
            return Ok(());
        }
        // 这一轮只用来回应插话，不再允许调工具，避免又拖出长链条
        stream_once(app, provider, messages, &[], stream_id).await?;
    }
    Ok(())
}

async fn wrap_up(
    app: &AppHandle,
    provider: &ProviderConfig,
    messages: &[ChatMessage],
    stream_id: &str,
) -> Result<(), String> {
    let mut final_messages = messages.to_vec();
    final_messages.push(ChatMessage::text(
        "user",
        "工具调用次数已达上限，不能再继续调用工具了。请根据已经获得的信息直接给出最终答复；\
         如果任务尚未完成，请说明已完成的部分、还差什么，以及建议用户下一步怎么做。",
    ));
    stream_once(app, provider, &final_messages, &[], stream_id).await?;
    Ok(())
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
    fn counts_repeated_calls() {
        let seen = vec!["a(1)".to_string(), "b(2)".to_string(), "a(1)".to_string()];
        assert_eq!(repeats_of(&seen, "a(1)"), 2);
        assert_eq!(repeats_of(&seen, "b(2)"), 1);
        assert_eq!(repeats_of(&seen, "c(3)"), 0);
    }

    #[test]
    fn queued_input_preserves_order_and_is_consumed_once() {
        let queue = QueuedInput::default();
        queue.push("s1", "第一条".into());
        queue.push("s1", "第二条".into());
        assert_eq!(queue.take("s1"), vec!["第一条", "第二条"]);
        // 取出后即清空，不会重复注入
        assert!(queue.take("s1").is_empty());
    }

    #[test]
    fn queued_input_is_isolated_per_stream() {
        let queue = QueuedInput::default();
        queue.push("s1", "给第一条流".into());
        queue.push("s2", "给第二条流".into());
        assert_eq!(queue.take("s1"), vec!["给第一条流"]);
        assert_eq!(queue.take("s2"), vec!["给第二条流"]);
    }

    #[test]
    fn drop_stream_discards_leftovers() {
        let queue = QueuedInput::default();
        queue.push("s1", "没赶上".into());
        queue.drop_stream("s1");
        assert!(queue.take("s1").is_empty());
    }

    #[test]
    fn late_input_drain_reports_whether_anything_was_there() {
        // answer_late_input 靠这个返回值决定要不要再请求一轮
        let queue = QueuedInput::default();
        assert!(queue.take("s1").is_empty());
        queue.push("s1", "迟到的话".into());
        assert_eq!(queue.take("s1"), vec!["迟到的话"]);
        // 只取一次，第二轮不会再触发请求
        assert!(queue.take("s1").is_empty());
    }
    #[test]
    fn input_accepted_payload_uses_camel_case() {
        let value = serde_json::to_value(InputAcceptedPayload {
            stream_id: "abc".into(),
            text: "补充一句".into(),
        })
        .unwrap();
        assert_eq!(value["streamId"], "abc");
        assert_eq!(value["text"], "补充一句");
    }
}
