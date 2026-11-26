use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

const DEFAULT_PROVIDER: &str = "deepseek";
const DEFAULT_BASE_URL: &str = "https://api.deepseek.com/v1";
const DEFAULT_MODEL: &str = "deepseek-chat";

fn default_provider() -> String {
    DEFAULT_PROVIDER.into()
}

fn default_base_url() -> String {
    DEFAULT_BASE_URL.into()
}

fn default_model() -> String {
    DEFAULT_MODEL.into()
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfig {
    #[serde(default = "default_provider")]
    provider: String,
    #[serde(default = "default_base_url")]
    base_url: String,
    #[serde(default = "default_model")]
    model: String,
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    thinking: bool,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            base_url: default_base_url(),
            model: default_model(),
            api_key: String::new(),
            thinking: false,
        }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfigView {
    provider: String,
    base_url: String,
    model: String,
    thinking: bool,
    has_api_key: bool,
    api_key_hint: String,
}

fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.is_empty() {
        String::new()
    } else if chars.len() <= 4 {
        "••••".into()
    } else {
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("••••{tail}")
    }
}

impl From<&ModelConfig> for ModelConfigView {
    fn from(config: &ModelConfig) -> Self {
        Self {
            provider: config.provider.clone(),
            base_url: config.base_url.clone(),
            model: config.model.clone(),
            thinking: config.thinking,
            has_api_key: !config.api_key.is_empty(),
            api_key_hint: mask_key(&config.api_key),
        }
    }
}

pub struct ModelSettings(Mutex<ModelConfig>);

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    crate::app::paths::config_dir(app).map(|path| path.join("models.json"))
}

fn validate(config: &ModelConfig) -> Result<(), String> {
    if !["deepseek", "custom"].contains(&config.provider.as_str()) {
        return Err("未知的模型服务商".into());
    }
    if !(config.base_url.starts_with("https://") || config.base_url.starts_with("http://")) {
        return Err("接口地址必须以 http:// 或 https:// 开头".into());
    }
    if config.model.trim().is_empty() {
        return Err("请填写模型名称".into());
    }
    Ok(())
}

fn save_file(app: &AppHandle, config: &ModelConfig) -> Result<(), String> {
    let path = config_path(app)?;
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(config).map_err(|error| error.to_string())?;
    fs::write(path, bytes).map_err(|error| error.to_string())
}

impl ModelSettings {
    pub fn load(app: &AppHandle) -> Self {
        let config = config_path(app)
            .ok()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<ModelConfig>(&bytes).ok())
            .filter(|config| validate(config).is_ok())
            .unwrap_or_default();
        Self(Mutex::new(config))
    }
}

pub struct RequestConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub thinking: bool,
}

pub fn snapshot(app: &AppHandle) -> Result<RequestConfig, String> {
    let state = app.state::<ModelSettings>();
    let config = state.0.lock().map_err(|error| error.to_string())?;
    Ok(RequestConfig {
        base_url: config.base_url.clone(),
        model: config.model.clone(),
        api_key: config.api_key.clone(),
        thinking: config.thinking,
    })
}

#[tauri::command]
pub fn model_config_path(app: AppHandle) -> Result<String, String> {
    config_path(&app).map(|path| path.display().to_string())
}

#[tauri::command]
pub fn get_model_config(state: tauri::State<ModelSettings>) -> Result<ModelConfigView, String> {
    state
        .0
        .lock()
        .map(|config| ModelConfigView::from(&*config))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_model_config(
    app: AppHandle,
    provider: String,
    base_url: String,
    model: String,
    thinking: bool,
    api_key: Option<String>,
    state: tauri::State<ModelSettings>,
) -> Result<ModelConfigView, String> {
    let mut current = state.0.lock().map_err(|error| error.to_string())?;
    let mut next = current.clone();
    next.provider = provider.trim().to_lowercase();
    next.base_url = base_url.trim().trim_end_matches('/').to_string();
    next.model = model.trim().to_string();
    next.thinking = thinking;
    if let Some(key) = api_key {
        let key = key.trim();
        if !key.is_empty() {
            next.api_key = key.to_string();
        }
    }
    validate(&next)?;
    save_file(&app, &next)?;
    *current = next.clone();
    Ok(ModelConfigView::from(&next))
}

const TEST_TIMEOUT_SECS: u64 = 20;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTestResult {
    pub elapsed_ms: u64,
    pub model: String,
    pub reply: String,
}

fn is_local_endpoint(base_url: &str) -> bool {
    let base = base_url.to_ascii_lowercase();
    base.contains("localhost") || base.contains("127.0.0.1") || base.contains("0.0.0.0")
}

fn shorten(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    let mut kept: String = trimmed.chars().take(limit).collect();
    if trimmed.chars().count() > limit {
        kept.push('…');
    }
    kept
}

/// 表单里的密钥可能为空（界面显示的是掩码）；此时回落到已保存的密钥。
fn pick_api_key(typed: Option<String>, saved: String) -> String {
    typed
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or(saved)
}

/// 用当前表单里的配置发一次最小请求，确认地址、模型名和密钥都能用。
/// 只做连通性验证，不写入配置，也不开启思考模式，尽量少消耗额度。
#[tauri::command]
pub async fn test_model_config(
    provider: String,
    base_url: String,
    model: String,
    api_key: Option<String>,
    state: tauri::State<'_, ModelSettings>,
) -> Result<ModelTestResult, String> {
    let saved_key = state
        .0
        .lock()
        .map(|config| config.api_key.clone())
        .map_err(|error| error.to_string())?;
    let key = pick_api_key(api_key, saved_key);

    let candidate = ModelConfig {
        provider: provider.trim().to_lowercase(),
        base_url: base_url.trim().trim_end_matches('/').to_string(),
        model: model.trim().to_string(),
        api_key: key.clone(),
        thinking: false,
    };
    validate(&candidate)?;
    if key.is_empty() && !is_local_endpoint(&candidate.base_url) {
        return Err("尚未设置 API Key，请先填写再测试".into());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(TEST_TIMEOUT_SECS))
        .build()
        .map_err(|error| format!("无法创建请求：{error}"))?;
    let body = serde_json::json!({
        "model": candidate.model,
        "messages": [{ "role": "user", "content": "你好" }],
        "stream": false,
        "max_tokens": 16,
    });
    let url = format!("{}/chat/completions", candidate.base_url);
    let mut request = client.post(&url).json(&body);
    if !key.is_empty() {
        request = request.bearer_auth(&key);
    }

    let started = std::time::Instant::now();
    eprintln!("[model-test] POST {url} model={}", candidate.model);
    let response = request.send().await.map_err(|error| {
        eprintln!("[model-test] 发送失败: {error}");
        if error.is_timeout() {
            format!("连接超时（{TEST_TIMEOUT_SECS} 秒），请检查接口地址与网络")
        } else {
            format!("无法连接 {url}：{error}")
        }
    })?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let elapsed_ms = started.elapsed().as_millis() as u64;
    eprintln!("[model-test] 响应 {status} 用时 {elapsed_ms}ms");

    if !status.is_success() {
        return Err(format!(
            "请求失败（{}）：{}",
            status.as_u16(),
            crate::agent::providers::extract_error_message(&text)
        ));
    }
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| {
        "服务返回的内容不是 JSON，请确认接口地址指向 OpenAI 兼容服务".to_string()
    })?;
    let reply = value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .unwrap_or("");
    let used_model = value
        .get("model")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(&candidate.model)
        .to_string();

    Ok(ModelTestResult {
        elapsed_ms,
        model: used_model,
        reply: if reply.trim().is_empty() {
            "服务已响应".into()
        } else {
            shorten(reply, 60)
        },
    })
}

#[tauri::command]
pub fn clear_model_api_key(
    app: AppHandle,
    state: tauri::State<ModelSettings>,
) -> Result<ModelConfigView, String> {
    let mut current = state.0.lock().map_err(|error| error.to_string())?;
    let mut next = current.clone();
    next.api_key.clear();
    save_file(&app, &next)?;
    *current = next.clone();
    Ok(ModelConfigView::from(&next))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_key_wins_and_blank_falls_back_to_saved() {
        assert_eq!(pick_api_key(Some("  sk-new ".into()), "sk-old".into()), "sk-new");
        assert_eq!(pick_api_key(Some("   ".into()), "sk-old".into()), "sk-old");
        assert_eq!(pick_api_key(None, "sk-old".into()), "sk-old");
        assert_eq!(pick_api_key(None, String::new()), "");
    }

    #[test]
    fn local_endpoints_do_not_need_a_key() {
        assert!(is_local_endpoint("http://localhost:11434/v1"));
        assert!(is_local_endpoint("http://127.0.0.1:1234/v1"));
        assert!(!is_local_endpoint("https://api.deepseek.com/v1"));
    }

    #[test]
    fn test_result_serializes_camel_case() {
        let value = serde_json::to_value(ModelTestResult {
            elapsed_ms: 812,
            model: "deepseek-chat".into(),
            reply: "嗨".into(),
        })
        .unwrap();
        assert_eq!(value["elapsedMs"], 812);
        assert_eq!(value["model"], "deepseek-chat");
        assert_eq!(value["reply"], "嗨");
        assert!(value.get("elapsed_ms").is_none());
    }

    #[test]
    fn short_replies_are_not_truncated() {
        assert_eq!(shorten("  嗨  ", 60), "嗨");
        assert_eq!(shorten("一二三四", 3), "一二三…");
    }

    #[test]
    fn invalid_candidates_are_rejected_before_requesting() {
        let bad_scheme = ModelConfig {
            provider: "deepseek".into(),
            base_url: "api.deepseek.com".into(),
            model: "deepseek-chat".into(),
            api_key: "sk".into(),
            thinking: false,
        };
        assert!(validate(&bad_scheme).is_err());

        let no_model = ModelConfig {
            base_url: "https://api.deepseek.com/v1".into(),
            model: "  ".into(),
            ..bad_scheme
        };
        assert!(validate(&no_model).is_err());
    }
}

