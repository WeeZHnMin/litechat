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
    crate::paths::config_dir(app).map(|path| path.join("models.json"))
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
