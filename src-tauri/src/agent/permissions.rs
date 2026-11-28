use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

pub const READ_ONLY_TOOLS: [&str; 10] = [
    "read_file",
    "list_dir",
    "search",
    "workspace_info",
    "list_tools",
    "read_binary_info",
    "read_document",
    "extract_pdf_images",
    "render_pdf_pages",
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
    pending: Option<Pending>,
}

/// 待确认的请求由后端自己记录工具名与归属服务，
/// 不采信前端回传的值——否则渲染层可以谎报工具名解锁另一个工具。
struct Pending {
    id: String,
    tool: String,
    server: String,
    sender: tokio::sync::oneshot::Sender<bool>,
}

/// 白名单键必须带服务前缀：不同服务可以有同名工具，
/// 只按裸名字记录会让 A 服务的授权顺带放开 B 服务。
fn permission_key(server: &str, tool: &str) -> String {
    format!("{server}__{tool}")
}

/// 旧版只存裸工具名，读出来补上服务前缀；
/// 否则升级后已有的「不再询问」全部失效。
fn migrate_key(stored: String) -> String {
    if stored.contains("__") {
        stored
    } else {
        permission_key(crate::tools::mcp::BUILTIN_SERVER, &stored)
    }
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
                always_allowed: stored
                    .always_allowed
                    .into_iter()
                    .map(migrate_key)
                    .collect(),
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

/// 无论哪种审批模式，这些工具都必须逐个确认。
/// 放这里的是不可撤销的操作——删掉就没了，不能靠「全部允许」跳过。
pub const ALWAYS_CONFIRM_TOOLS: [&str; 1] = ["delete_files"];

fn needs_confirmation_every_time(tool: &str, server: &str) -> bool {
    server == crate::tools::mcp::BUILTIN_SERVER && ALWAYS_CONFIRM_TOOLS.contains(&tool)
}

/// 只有内置服务的工具走显式白名单免审批。
/// 外部 MCP 服务的工具名不可信，一律不自动放行——
/// 恶意服务可以把破坏性工具命名为 read_* 来绕过前缀规则。
fn is_read_only(tool: &str, server: &str) -> bool {
    server == crate::tools::mcp::BUILTIN_SERVER && READ_ONLY_TOOLS.contains(&tool)
}

pub(crate) fn decide(inner: &Inner, tool: &str, server: &str) -> Decision {
    // 不可撤销的操作不看模式，也不看白名单
    if needs_confirmation_every_time(tool, server) {
        return Decision::Ask;
    }
    if is_read_only(tool, server) {
        return Decision::Allow;
    }
    let key = permission_key(server, tool);
    let allowed = match inner.mode {
        ApprovalMode::Always => true,
        ApprovalMode::Session => inner.session_allowed.contains(&key),
        ApprovalMode::Ask => inner.always_allowed.contains(&key),
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

    pub fn allow_always(&self, server: &str, tool: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.always_allowed.insert(permission_key(server, tool));
        }
        self.save();
    }

    fn needs_approval(&self, tool: &str, server: &str) -> bool {
        let Ok(inner) = self.inner.lock() else {
            return true;
        };
        decide(&inner, tool, server) == Decision::Ask
    }

    fn allow_session(&self, server: &str, tool: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.session_allowed.insert(permission_key(server, tool));
        }
    }

    /// 「不再询问」要记进当前模式真正会查的那个集合。
    /// Ask 模式查 always_allowed、Session 模式查 session_allowed，
    /// 记错集合会导致按钮点了没反应。
    fn remember_in_current_mode(&self, server: &str, tool: &str) {
        let mode = match self.inner.lock() {
            Ok(inner) => inner.mode,
            Err(_) => return,
        };
        match mode {
            ApprovalMode::Ask => self.allow_always(server, tool),
            ApprovalMode::Session => self.allow_session(server, tool),
            ApprovalMode::Always => {}
        }
    }

    fn clear_pending(&self, id: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            if inner.pending.as_ref().map(|item| item.id == id).unwrap_or(false) {
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
        if let Some(stale) = inner.pending.take() {
            eprintln!("[permission] 丢弃过期的确认请求: {}", stale.id);
            let _ = stale.sender.send(false);
        }
        inner.pending = Some(Pending {
            id: id.clone(),
            tool: tool.to_string(),
            server: server.to_string(),
            sender: tx,
        });
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
) -> Result<(), String> {
    let state = app.state::<Permissions>();
    // 工具名与归属服务取自后端记录的待确认请求，不用前端传参
    let (tool, server) = {
        let mut inner = state.inner.lock().map_err(|error| error.to_string())?;
        let Some(pending) = inner.pending.take() else {
            return Err("没有待确认的请求".into());
        };
        if pending.id != id {
            inner.pending = Some(pending);
            return Err("请求已过期".into());
        }
        let _ = pending.sender.send(allowed);
        (pending.tool, pending.server)
    };
    if allowed
        && !needs_confirmation_every_time(&tool, &server)
        && remember.as_deref() == Some("session")
    {
        state.remember_in_current_mode(&server, &tool);
    }
    Ok(())
}

impl Permissions {
    pub fn begin_turn(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(pending) = inner.pending.take() {
                let _ = pending.sender.send(false);
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
        assert!(is_read_only("extract_pdf_images", "shell"));
        assert!(is_read_only("render_pdf_pages", "shell"));
        assert!(is_read_only("read_binary_info", "shell"));
        assert!(is_read_only("read_document", "shell"));
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
        permissions.allow_session("shell", "run_command");
        assert!(!permissions.needs_approval("run_command", "shell"));
        assert!(permissions.needs_approval("write_file", "shell"));
    }

    #[test]
    fn ask_mode_honors_always_allowed_list() {
        let permissions = Permissions::in_memory();
        assert!(permissions.needs_approval("write_file", "shell"));
        permissions.allow_always("shell", "write_file");
        assert!(!permissions.needs_approval("write_file", "shell"));
        assert!(permissions.needs_approval("run_command", "shell"));
    }

    #[test]
    fn approvals_do_not_leak_across_servers() {
        let permissions = Permissions::in_memory();
        permissions.allow_always("shell", "write_file");
        assert!(!permissions.needs_approval("write_file", "shell"));
        // 同名工具挂在别的服务上不继承授权
        assert!(permissions.needs_approval("write_file", "filesystem"));
    }

    #[test]
    fn legacy_bare_keys_gain_a_server_prefix() {
        assert_eq!(migrate_key("write_file".into()), "shell__write_file");
        // 已经是带前缀的键原样保留
        assert_eq!(
            migrate_key("filesystem__read_text_file".into()),
            "filesystem__read_text_file"
        );
    }

    #[test]
    fn remember_button_works_in_every_mode() {
        // 「此工具不再询问」发的是 remember=session，
        // 必须记进当前模式真正会查的集合，否则按钮点了没效果
        for mode in [ApprovalMode::Ask, ApprovalMode::Session] {
            let permissions = Permissions::in_memory();
            permissions.set_mode(mode);
            assert!(permissions.needs_approval("write_file", "shell"));
            permissions.remember_in_current_mode("shell", "write_file");
            assert!(
                !permissions.needs_approval("write_file", "shell"),
                "{mode:?} 模式下记住后不应再问"
            );
        }
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
        permissions.allow_session("shell", "write_file");
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
    fn external_tools_are_never_auto_allowed() {
        let inner = fresh(ApprovalMode::default());
        // 外部服务的工具名不可信，即使叫 read_* / list_* 也要确认
        for tool in ["read_text_file", "list_directory", "get_pods", "search_nodes"] {
            assert_eq!(decide(&inner, tool, "filesystem"), Decision::Ask, "{tool} 必须确认");
        }
        assert_eq!(decide(&inner, "create_entities", "memory"), Decision::Ask);
        assert_eq!(decide(&inner, "write_file", "filesystem"), Decision::Ask);
    }

    #[test]
    fn external_tools_need_explicit_allow() {
        let mut inner = fresh(ApprovalMode::default());
        inner
            .always_allowed
            .insert(permission_key("filesystem", "read_text_file"));
        assert_eq!(decide(&inner, "read_text_file", "filesystem"), Decision::Allow);
        assert_eq!(decide(&inner, "list_directory", "filesystem"), Decision::Ask);
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
    fn delete_always_requires_approval_in_every_mode() {
        for mode in [ApprovalMode::Ask, ApprovalMode::Session, ApprovalMode::Always] {
            let mut inner = fresh(mode);
            // 即使把它记进白名单也不放行
            inner
                .always_allowed
                .insert(permission_key("shell", "delete_files"));
            inner
                .session_allowed
                .insert(permission_key("shell", "delete_files"));
            assert_eq!(
                decide(&inner, "delete_files", "shell"),
                Decision::Ask,
                "{mode:?} 模式下删除也必须确认"
            );
        }
    }

    #[test]
    fn delete_is_not_treated_as_read_only() {
        assert!(!is_read_only("delete_files", "shell"));
    }

    #[test]
    fn switching_mode_clears_other_scope() {
        let permissions = Permissions::in_memory();
        permissions.set_mode(ApprovalMode::Session);
        permissions.allow_session("shell", "run_command");
        permissions.set_mode(ApprovalMode::Ask);
        assert!(permissions.needs_approval("run_command", "shell"));
    }
}
