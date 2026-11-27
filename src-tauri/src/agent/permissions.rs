use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

pub const READ_ONLY_TOOLS: [&str; 8] = [
    "read_file",
    "list_dir",
    "search",
    "workspace_info",
    "list_tools",
    "get_file_info",
    "read_multiple_text_files",
    "read_media_file",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalMode {
    Ask,
    Session,
    Always,
}

impl Default for ApprovalMode {
    fn default() -> Self {
        Self::Ask
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingApproval {
    pub id: String,
    pub tool: String,
    pub server: String,
    pub arguments: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionState {
    pub mode: ApprovalMode,
    pub always_allowed: Vec<String>,
}

pub(crate) struct Inner {
    mode: ApprovalMode,
    always_allowed: HashSet<String>,
    session_allowed: HashSet<String>,
    pending: Option<(String, tokio::sync::oneshot::Sender<bool>)>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct StoredPrefs {
    mode: ApprovalMode,
    #[serde(default)]
    always_allowed: Vec<String>,
}

pub struct Permissions {
    inner: Mutex<Inner>,
    path: std::path::PathBuf,
}

impl Permissions {
    pub fn load(app: &AppHandle) -> Self {
        let path = crate::app::paths::config_dir(app)
            .map(|dir| dir.join("permissions.json"))
            .unwrap_or_else(|_| std::path::PathBuf::from("permissions.json"));
        let stored = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<StoredPrefs>(&bytes).ok())
            .unwrap_or_default();
        Self {
            inner: Mutex::new(Inner {
                mode: stored.mode,
                always_allowed: stored.always_allowed.into_iter().collect(),
                session_allowed: HashSet::new(),
                pending: None,
            }),
            path,
        }
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self {
            inner: Mutex::new(Inner {
                mode: ApprovalMode::default(),
                always_allowed: HashSet::new(),
                session_allowed: HashSet::new(),
                pending: None,
            }),
            path: std::env::temp_dir().join("litechat-permissions-test.json"),
        }
    }

    fn save(&self) {
        let prefs = match self.inner.lock() {
            Ok(inner) => StoredPrefs {
                mode: inner.mode,
                always_allowed: inner.always_allowed.iter().cloned().collect(),
            },
            Err(_) => return,
        };
        if let Some(directory) = self.path.parent() {
            let _ = std::fs::create_dir_all(directory);
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(&prefs) {
            let _ = std::fs::write(&self.path, bytes);
        }
    }
}

fn is_read_only(tool: &str, server: &str) -> bool {
    if server == crate::tools::mcp::BUILTIN_SERVER {
        if READ_ONLY_TOOLS.contains(&tool) {
            return true;
        }
    }
    tool.starts_with("read_")
        || tool.starts_with("list_")
        || tool.starts_with("get_")
        || tool.starts_with("search")
        || tool == "memory__search_nodes"
        || tool == "memory__open_nodes"
        || tool == "memory__read_graph"
}

pub(crate) fn decide(inner: &Inner, tool: &str, server: &str) -> Decision {
    if is_read_only(tool, server) {
        return Decision::Allow;
    }
    let allowed = match inner.mode {
        ApprovalMode::Always => true,
        ApprovalMode::Session => inner.session_allowed.contains(tool),
        ApprovalMode::Ask => inner.always_allowed.contains(tool),
    };
    if allowed {
        Decision::Allow
    } else {
        Decision::Ask
    }
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum Decision {
    Allow,
    Ask,
}

impl Permissions {
    pub fn state(&self) -> PermissionState {
        let inner = self.inner.lock().ok();
        let (mode, always) = inner
            .as_ref()
            .map(|guard| (guard.mode, guard.always_allowed.iter().cloned().collect()))
            .unwrap_or((ApprovalMode::Ask, Vec::new()));
        PermissionState {
            mode,
            always_allowed: always,
        }
    }

    pub fn set_mode(&self, mode: ApprovalMode) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.mode = mode;
            if mode != ApprovalMode::Always {
                inner.always_allowed.clear();
            }
            if mode != ApprovalMode::Session {
                inner.session_allowed.clear();
            }
        }
        self.save();
    }

    pub fn allow_always(&self, tool: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.always_allowed.insert(tool.to_string());
        }
        self.save();
    }

    fn needs_approval(&self, tool: &str, server: &str) -> bool {
        let Ok(inner) = self.inner.lock() else {
            return true;
        };
        decide(&inner, tool, server) == Decision::Ask
    }

    fn allow_session(&self, tool: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.session_allowed.insert(tool.to_string());
        }
    }

    fn clear_pending(&self, id: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            if inner.pending.as_ref().map(|(pid, _)| pid == id).unwrap_or(false) {
                inner.pending = None;
            }
        }
    }
}

pub async fn ensure_allowed(
    app: &AppHandle,
    tool: &str,
    server: &str,
    arguments: &str,
) -> Result<(), String> {
    let state = app.state::<Permissions>();
    if !state.needs_approval(tool, server) {
        return Ok(());
    }
    eprintln!("[permission] 等待确认: {server}__{tool}");
    let id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = tokio::sync::oneshot::channel();
    {
        let mut inner = state.inner.lock().map_err(|error| error.to_string())?;
        if let Some((stale_id, stale)) = inner.pending.take() {
            eprintln!("[permission] 丢弃过期的确认请求: {stale_id}");
            let _ = stale.send(false);
        }
        inner.pending = Some((id.clone(), tx));
    }
    let _ = app.emit(
        "chat://approval-request",
        PendingApproval {
            id: id.clone(),
            tool: tool.to_string(),
            server: server.to_string(),
            arguments: arguments.to_string(),
        },
    );
    let decision = tokio::time::timeout(std::time::Duration::from_secs(120), rx).await;
    state.clear_pending(&id);
    match decision {
        Ok(Ok(true)) => {
            eprintln!("[permission] 用户允许: {server}__{tool}");
            Ok(())
        }
        Ok(Ok(false)) => {
            eprintln!("[permission] 用户拒绝: {server}__{tool}");
            Err("用户拒绝了这次工具调用".into())
        }
        _ => {
            eprintln!("[permission] 超时拒绝: {server}__{tool}");
            Err("等待用户确认超时，已取消".into())
        }
    }
}

#[tauri::command]
pub fn respond_approval(
    app: AppHandle,
    id: String,
    allowed: bool,
    remember: Option<String>,
    tool: String,
) -> Result<(), String> {
    let state = app.state::<Permissions>();
    {
        let mut inner = state.inner.lock().map_err(|error| error.to_string())?;
        let Some((pending_id, sender)) = inner.pending.take() else {
            return Err("没有待确认的请求".into());
        };
        if pending_id != id {
            inner.pending = Some((pending_id, sender));
            return Err("请求已过期".into());
        }
        let _ = sender.send(allowed);
    }
    if allowed {
        match remember.as_deref() {
            Some("session") => state.allow_session(&tool),
            Some("always") => state.allow_always(&tool),
            _ => {}
        }
    }
    Ok(())
}

impl Permissions {
    pub fn begin_turn(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some((_, sender)) = inner.pending.take() {
                let _ = sender.send(false);
            }
        }
    }
}

#[tauri::command]
pub fn get_permission_state(app: AppHandle) -> Result<PermissionState, String> {
    Ok(app.state::<Permissions>().state())
}

#[tauri::command]
pub fn set_approval_mode(app: AppHandle, mode: ApprovalMode) -> Result<PermissionState, String> {
    let state = app.state::<Permissions>();
    state.set_mode(mode);
    Ok(state.state())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_tools_skip_approval() {
        assert!(is_read_only("read_file", "shell"));
        assert!(is_read_only("list_dir", "shell"));
        assert!(is_read_only("search", "shell"));
        assert!(is_read_only("read_text_file", "filesystem"));
        assert!(!is_read_only("write_file", "shell"));
        assert!(!is_read_only("run_command", "shell"));
        assert!(!is_read_only("edit_file", "shell"));
    }

    #[test]
    fn always_mode_allows_everything() {
        let permissions = Permissions::in_memory();
        assert!(permissions.needs_approval("write_file", "shell"));
        permissions.set_mode(ApprovalMode::Always);
        assert!(!permissions.needs_approval("write_file", "shell"));
        assert!(!permissions.needs_approval("run_command", "shell"));
    }

    #[test]
    fn session_mode_remembers_approved_tools() {
        let permissions = Permissions::in_memory();
        permissions.set_mode(ApprovalMode::Session);
        assert!(permissions.needs_approval("run_command", "shell"));
        permissions.allow_session("run_command");
        assert!(!permissions.needs_approval("run_command", "shell"));
        assert!(permissions.needs_approval("write_file", "shell"));
    }

    #[test]
    fn ask_mode_honors_always_allowed_list() {
        let permissions = Permissions::in_memory();
        assert!(permissions.needs_approval("write_file", "shell"));
        permissions.allow_always("write_file");
        assert!(!permissions.needs_approval("write_file", "shell"));
        assert!(permissions.needs_approval("run_command", "shell"));
    }

    fn fresh(mode: ApprovalMode) -> Inner {
        Inner {
            mode,
            always_allowed: HashSet::new(),
            session_allowed: HashSet::new(),
            pending: None,
        }
    }

    #[test]
    fn prefs_survive_serialization() {
        let prefs = StoredPrefs {
            mode: ApprovalMode::Always,
            always_allowed: vec!["write_file".into()],
        };
        let bytes = serde_json::to_vec(&prefs).unwrap();
        let restored: StoredPrefs = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.mode, ApprovalMode::Always);
        assert_eq!(restored.always_allowed, vec!["write_file".to_string()]);
    }

    #[test]
    fn missing_or_corrupt_prefs_fall_back_to_ask() {
        let default = StoredPrefs::default();
        assert_eq!(default.mode, ApprovalMode::Ask);
        assert!(default.always_allowed.is_empty());
        assert!(serde_json::from_slice::<StoredPrefs>(b"not json").is_err());
    }

    #[test]
    fn session_allow_is_per_tool() {
        let permissions = Permissions::in_memory();
        permissions.set_mode(ApprovalMode::Session);
        permissions.allow_session("write_file");
        assert!(!permissions.needs_approval("write_file", "shell"));
        assert!(permissions.needs_approval("run_command", "shell"));
    }

    #[test]
    fn default_state_requires_approval_for_mutating_tools() {
        let inner = fresh(ApprovalMode::default());
        for tool in [
            "write_file",
            "edit_file",
            "run_command",
            "run_shell",
            "add_mcp_server",
            "remove_mcp_server",
        ] {
            assert_eq!(decide(&inner, tool, "shell"), Decision::Ask, "{tool} 必须确认");
        }
    }

    #[test]
    fn only_read_only_tools_bypass_by_default() {
        let inner = fresh(ApprovalMode::default());
        for tool in READ_ONLY_TOOLS {
            assert_eq!(decide(&inner, tool, "shell"), Decision::Allow, "{tool} 应免确认");
        }
    }

    #[test]
    fn external_write_tools_require_approval() {
        let inner = fresh(ApprovalMode::default());
        assert_eq!(decide(&inner, "create_entities", "memory"), Decision::Ask);
        assert_eq!(decide(&inner, "search_nodes", "memory"), Decision::Allow);
        assert_eq!(decide(&inner, "read_text_file", "filesystem"), Decision::Allow);
        assert_eq!(decide(&inner, "write_file", "filesystem"), Decision::Ask);
    }

    #[test]
    fn ask_and_session_modes_deny_until_explicitly_allowed() {
        for mode in [ApprovalMode::Ask, ApprovalMode::Session] {
            let inner = fresh(mode);
            assert_eq!(decide(&inner, "run_command", "shell"), Decision::Ask, "{mode:?}");
        }
    }

    #[test]
    fn never_asks_in_always_mode() {
        let inner = fresh(ApprovalMode::Always);
        for tool in ["write_file", "run_command", "add_mcp_server"] {
            assert_eq!(decide(&inner, tool, "shell"), Decision::Allow);
        }
    }

    #[test]
    fn switching_mode_clears_other_scope() {
        let permissions = Permissions::in_memory();
        permissions.set_mode(ApprovalMode::Session);
        permissions.allow_session("run_command");
        permissions.set_mode(ApprovalMode::Ask);
        assert!(permissions.needs_approval("run_command", "shell"));
    }
}
