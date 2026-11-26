use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

#[derive(Clone, Serialize)]
pub struct ImageUrl {
    pub url: String,
}

#[derive(Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn text(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: Some(Content::Text(content.into())),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn parts(role: impl Into<String>, parts: Vec<ContentPart>) -> Self {
        Self {
            role: role.into(),
            content: Some(Content::Parts(parts)),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn tool_result(tool_call_id: String, content: String) -> Self {
        Self {
            role: "tool".into(),
            content: Some(Content::Text(content)),
            tool_calls: None,
            tool_call_id: Some(tool_call_id),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolFunction,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Clone, Serialize)]
pub struct ToolSpec {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolFunctionSpec,
}

#[derive(Clone, Serialize)]
pub struct ToolFunctionSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

pub fn tool_spec(name: String, description: String, parameters: serde_json::Value) -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: ToolFunctionSpec {
            name,
            description,
            parameters,
        },
    }
}

#[derive(Clone)]
pub struct ProviderConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    /// OpenAI 协议的推理强度：off / low / medium / high。
    pub reasoning_effort: String,
}

pub enum StreamEvent {
    Delta(String),
    Reasoning(String),
    ToolCalls(Vec<ToolCall>),
    Done,
}

pub enum ProviderError {
    MissingApiKey,
    Http(u16, String),
    Transport(String),
}

impl ProviderError {
    pub fn message(&self) -> String {
        match self {
            Self::MissingApiKey => "尚未设置 API Key，请先在设置中填写".into(),
            Self::Http(status, body) => {
                let detail = extract_error_message(body);
                format!("请求失败 ({status})：{detail}")
            }
            Self::Transport(error) => format!("网络错误：{error}"),
        }
    }
}

pub(crate) fn extract_error_message(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "服务未返回错误详情".into();
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(message) = value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(|message| message.as_str())
        {
            return message.to_string();
        }
        if let Some(message) = value.get("message").and_then(|message| message.as_str()) {
            return message.to_string();
        }
    }
    trimmed.chars().take(300).collect()
}

fn read_sse_data(line: &str) -> Option<&str> {
    line.strip_prefix("data:").map(|data| data.trim())
}

#[derive(Default)]
pub struct ChunkDelta {
    pub content: Option<String>,
    pub reasoning: Option<String>,
    pub tool_calls: Vec<ToolCallFragment>,
}

pub struct ToolCallFragment {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub arguments: String,
}

pub fn chunk_delta(payload: &str) -> ChunkDelta {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        return ChunkDelta::default();
    };
    let Some(delta) = value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("delta"))
    else {
        return ChunkDelta::default();
    };
    ChunkDelta {
        content: delta
            .get("content")
            .and_then(|value| value.as_str())
            .filter(|text| !text.is_empty())
            .map(str::to_string),
        reasoning: delta
            .get("reasoning_content")
            .and_then(|value| value.as_str())
            .filter(|text| !text.is_empty())
            .map(str::to_string),
        tool_calls: delta
            .get("tool_calls")
            .and_then(|value| value.as_array())
            .map(|calls| {
                calls
                    .iter()
                    .enumerate()
                    .map(|(position, call)| ToolCallFragment {
                        index: call
                            .get("index")
                            .and_then(|value| value.as_u64())
                            .map(|value| value as usize)
                            .unwrap_or(position),
                        id: call
                            .get("id")
                            .and_then(|value| value.as_str())
                            .unwrap_or("")
                            .to_string(),
                        name: call
                            .get("function")
                            .and_then(|function| function.get("name"))
                            .and_then(|value| value.as_str())
                            .unwrap_or("")
                            .to_string(),
                        arguments: call
                            .get("function")
                            .and_then(|function| function.get("arguments"))
                            .and_then(|value| value.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

#[derive(Default)]
pub struct ToolCallAccumulator {
    entries: std::collections::BTreeMap<usize, ToolCall>,
    buffers: std::collections::BTreeMap<usize, String>,
}

impl ToolCallAccumulator {
    pub fn push(&mut self, fragments: &[ToolCallFragment]) {
        for fragment in fragments {
            let entry = self.entries.entry(fragment.index).or_insert_with(|| ToolCall {
                id: fragment.id.clone(),
                kind: "function".into(),
                function: ToolFunction {
                    name: fragment.name.clone(),
                    arguments: String::new(),
                },
            });
            if !fragment.id.is_empty() {
                entry.id = fragment.id.clone();
            }
            if !fragment.name.is_empty() {
                entry.function.name = fragment.name.clone();
            }
            if !fragment.arguments.is_empty() {
                self.buffers
                    .entry(fragment.index)
                    .or_default()
                    .push_str(&fragment.arguments);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn finish(mut self) -> Vec<ToolCall> {
        for (index, call) in self.entries.iter_mut() {
            if let Some(buffer) = self.buffers.get(index) {
                call.function.arguments = buffer.clone();
            }
            if call.id.is_empty() {
                call.id = format!("call_{index}");
            }
        }
        self.entries.into_values().collect()
    }
}

/// 按 OpenAI 的 chat/completions 协议拼请求体。
/// 推理强度用标准的 reasoning_effort 字段，off 时不发送。
fn build_body(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    tools: Vec<ToolSpec>,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": config.model,
        "messages": messages,
        "stream": true,
    });
    if !tools.is_empty() {
        body["tools"] = serde_json::to_value(&tools).unwrap_or_default();
        body["tool_choice"] = serde_json::json!("auto");
    }
    if config.reasoning_effort != "off" {
        body["reasoning_effort"] = serde_json::json!(config.reasoning_effort);
    }
    body
}

pub async fn stream_chat<F>(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    tools: Vec<ToolSpec>,
    mut on_event: F,
) -> Result<(), ProviderError>
where
    F: FnMut(StreamEvent) -> bool + Send,
{
    if config.api_key.trim().is_empty() && !is_local(config) {
        return Err(ProviderError::MissingApiKey);
    }
    let body = build_body(config, messages, tools);
    {
        let preview = serde_json::to_string(&body).unwrap_or_default();
        let clipped: String = preview.chars().take(1200).collect();
        eprintln!("[req] {clipped}");
    }
    let client = reqwest::Client::builder()
        .build()
        .map_err(|error| ProviderError::Transport(error.to_string()))?;
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let mut request = client.post(url).json(&body);
    if !config.api_key.trim().is_empty() {
        request = request.bearer_auth(config.api_key.trim());
    }
    let response = request
        .send()
        .await
        .map_err(|error| ProviderError::Transport(error.to_string()))?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        return Err(ProviderError::Http(status, text));
    }
    let mut stream = response.bytes_stream();
    let mut pending: Vec<u8> = Vec::new();
    let mut buffer = String::new();
    let mut calls = ToolCallAccumulator::default();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|error| ProviderError::Transport(error.to_string()))?;
        pending.extend_from_slice(&bytes);
        let mut consumed = 0;
        while consumed < pending.len() {
            match std::str::from_utf8(&pending[consumed..]) {
                Ok(text) => {
                    buffer.push_str(text);
                    consumed = pending.len();
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    buffer.push_str(
                        std::str::from_utf8(&pending[consumed..consumed + valid]).unwrap_or(""),
                    );
                    consumed += valid;
                    if error.error_len().is_none() {
                        break;
                    }
                    consumed += 1;
                }
            }
        }
        pending.drain(..consumed);
        while let Some(index) = buffer.find('\n') {
            let line = buffer[..index].trim_end_matches('\r').to_string();
            buffer.drain(..=index);
            let Some(payload) = read_sse_data(&line) else {
                continue;
            };
            if payload.is_empty() {
                continue;
            }
            if payload == "[DONE]" {
                if !calls.is_empty() {
                    on_event(StreamEvent::ToolCalls(calls.finish()));
                }
                on_event(StreamEvent::Done);
                return Ok(());
            }
            let delta = chunk_delta(payload);
            if let Some(reasoning) = delta.reasoning {
                if !on_event(StreamEvent::Reasoning(reasoning)) {
                    return Ok(());
                }
            }
            if let Some(content) = delta.content {
                if !on_event(StreamEvent::Delta(content)) {
                    return Ok(());
                }
            }
            if !delta.tool_calls.is_empty() {
                calls.push(&delta.tool_calls);
            }
        }
    }
    if !calls.is_empty() {
        on_event(StreamEvent::ToolCalls(calls.finish()));
    }
    on_event(StreamEvent::Done);
    Ok(())
}

fn is_local(config: &ProviderConfig) -> bool {
    let base = config.base_url.to_ascii_lowercase();
    base.contains("localhost") || base.contains("127.0.0.1") || base.contains("0.0.0.0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_delta() {
        let payload = r#"{"choices":[{"delta":{"content":"你好"}}]}"#;
        let delta = chunk_delta(payload);
        assert_eq!(delta.content, Some("你好".into()));
        assert_eq!(delta.reasoning, None);
    }

    #[test]
    fn parses_reasoning_delta() {
        let payload = r#"{"choices":[{"delta":{"reasoning_content":"思考中"}}]}"#;
        let delta = chunk_delta(payload);
        assert_eq!(delta.content, None);
        assert_eq!(delta.reasoning, Some("思考中".into()));
    }

    #[test]
    fn parses_both_fields_in_one_chunk() {
        let payload = r#"{"choices":[{"delta":{"content":"答","reasoning_content":"想"}}]}"#;
        let delta = chunk_delta(payload);
        assert_eq!(delta.content, Some("答".into()));
        assert_eq!(delta.reasoning, Some("想".into()));
    }

    #[test]
    fn ignores_empty_and_malformed_chunks() {
        for payload in [
            r#"{"choices":[{"delta":{"content":""}}]}"#,
            r#"{"choices":[{"delta":{}}]}"#,
            r#"{"choices":[]}"#,
            "not json",
        ] {
            let delta = chunk_delta(payload);
            assert_eq!(delta.content, None);
            assert_eq!(delta.reasoning, None);
            assert!(delta.tool_calls.is_empty());
        }
    }

    #[test]
    fn accumulates_streamed_tool_call_arguments() {
        let mut calls = ToolCallAccumulator::default();
        for payload in [
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","function":{"name":"read_file","arguments":"{\"pa"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"a.txt\"}"}}]}}]}"#,
        ] {
            let delta = chunk_delta(payload);
            calls.push(&delta.tool_calls);
        }
        let finished = calls.finish();
        assert_eq!(finished.len(), 1);
        assert_eq!(finished[0].id, "call_a");
        assert_eq!(finished[0].function.name, "read_file");
        assert_eq!(finished[0].function.arguments, r#"{"path":"a.txt"}"#);
    }

    #[test]
    fn keeps_parallel_tool_calls_separate() {
        let mut calls = ToolCallAccumulator::default();
        let payload = r#"{"choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"a","function":{"name":"one","arguments":"{}"}},
            {"index":1,"id":"b","function":{"name":"two","arguments":"{}"}}
        ]}}]}"#;
        calls.push(&chunk_delta(payload).tool_calls);
        let finished = calls.finish();
        assert_eq!(finished.len(), 2);
        assert_eq!(finished[0].function.name, "one");
        assert_eq!(finished[1].function.name, "two");
    }

    #[test]
    fn chat_message_omits_empty_fields() {
        let plain = serde_json::to_value(ChatMessage::text("user", "hi")).unwrap();
        assert_eq!(plain["role"], "user");
        assert_eq!(plain["content"], "hi");
        assert!(plain.get("tool_calls").is_none());
        assert!(plain.get("tool_call_id").is_none());

        let result = serde_json::to_value(ChatMessage::tool_result("call_a".into(), "ok".into())).unwrap();
        assert_eq!(result["role"], "tool");
        assert_eq!(result["tool_call_id"], "call_a");
        assert!(result.get("content").is_some());
    }

    #[test]
    fn utf8_char_split_across_chunks_is_not_corrupted() {
        let bytes = "要帮忙".as_bytes();
        let (head, tail) = bytes.split_at(1);
        assert!(std::str::from_utf8(head).is_err());

        let mut pending: Vec<u8> = Vec::new();
        let mut buffer = String::new();
        for chunk in [head, tail] {
            pending.extend_from_slice(chunk);
            let mut consumed = 0;
            while consumed < pending.len() {
                match std::str::from_utf8(&pending[consumed..]) {
                    Ok(text) => {
                        buffer.push_str(text);
                        consumed = pending.len();
                    }
                    Err(error) => {
                        let valid = error.valid_up_to();
                        buffer.push_str(
                            std::str::from_utf8(&pending[consumed..consumed + valid]).unwrap_or(""),
                        );
                        consumed += valid;
                        if error.error_len().is_none() {
                            break;
                        }
                        consumed += 1;
                    }
                }
            }
            pending.drain(..consumed);
        }
        assert_eq!(buffer, "要帮忙");
        assert!(!buffer.contains('\u{FFFD}'));
    }

    #[test]
    fn local_endpoints_skip_api_key() {
        let config = ProviderConfig {
            base_url: "http://localhost:11434/v1".into(),
            model: "qwen".into(),
            api_key: String::new(),
            reasoning_effort: "off".into(),
        };
        assert!(is_local(&config));
    }

    fn request_body(effort: &str) -> serde_json::Value {
        let config = ProviderConfig {
            base_url: "https://api.example.com/v1".into(),
            model: "m".into(),
            api_key: "sk".into(),
            reasoning_effort: effort.into(),
        };
        super::build_body(&config, Vec::new(), Vec::new())
    }

    #[test]
    fn effort_off_omits_the_field() {
        assert!(request_body("off").get("reasoning_effort").is_none());
    }

    #[test]
    fn effort_levels_use_openai_reasoning_effort() {
        for level in ["low", "medium", "high"] {
            let body = request_body(level);
            assert_eq!(body["reasoning_effort"], level, "{level}");
            // 不能再出现 Anthropic 风格的私有字段
            assert!(body.get("thinking").is_none(), "{level} 不应发送 thinking");
        }
    }
}
