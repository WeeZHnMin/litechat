use crate::agent::providers::ChatMessage;
use crate::app::storage::StoredMessage as StorageMessage;

pub const SYSTEM_PROMPT: &str = "你是一个乐于助人的助手。回答简洁准确，使用 Markdown 格式。

使用工具时请遵守：
1. 先想清楚要做什么，再一次性把必要的信息收集齐，避免零散地反复试探。
2. 同一个工具、同样的参数，不要重复调用。如果结果不如预期，先分析原因再换方案。
3. 工具返回失败时，读清楚错误信息；连续两次同类失败就停下来，向用户说明情况并询问，不要靠换工具名硬试。
4. 信息足够回答问题时就立即作答，不要为了「更完整」而继续调用工具。";

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


pub fn build_messages(stored: Vec<StorageMessage>) -> Vec<ChatMessage> {
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


#[cfg(test)]
mod tests {
    use super::*;

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
