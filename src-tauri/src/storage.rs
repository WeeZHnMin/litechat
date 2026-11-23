use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

#[derive(Clone, serde::Serialize)]
pub struct Conversation {
    id: String,
    title: String,
    created_at: i64,
    updated_at: i64,
}

#[derive(Clone, serde::Serialize)]
pub struct StoredMessage {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub parts: serde_json::Value,
    pub created_at: i64,
}

pub struct Storage {
    connection: std::sync::Mutex<Connection>,
}

fn timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn database_path(app: &AppHandle) -> Result<PathBuf, String> {
    crate::paths::config_dir(app).map(|path| path.join("history.db"))
}

impl Storage {
    pub fn initialize(app: &AppHandle) -> Result<Self, String> {
        let path = database_path(app)?;
        if let Some(directory) = path.parent() {
            std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        }
        let connection = Connection::open(path).map_err(|error| error.to_string())?;
        connection
            .busy_timeout(Duration::from_secs(3))
            .map_err(|error| error.to_string())?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 PRAGMA journal_mode = WAL;
                 CREATE TABLE IF NOT EXISTS conversations (
                     id TEXT PRIMARY KEY,
                     title TEXT NOT NULL,
                     created_at INTEGER NOT NULL,
                     updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS messages (
                     id TEXT PRIMARY KEY,
                     conversation_id TEXT NOT NULL,
                     role TEXT NOT NULL,
                     parts TEXT NOT NULL,
                     created_at INTEGER NOT NULL,
                     FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
                 );
                 CREATE INDEX IF NOT EXISTS messages_conversation_index
                 ON messages(conversation_id, created_at);",
            )
            .map_err(|error| error.to_string())?;
        Ok(Self {
            connection: std::sync::Mutex::new(connection),
        })
    }
}

#[tauri::command]
pub fn create_conversation(
    title: Option<String>,
    state: tauri::State<Storage>,
) -> Result<Conversation, String> {
    let now = timestamp();
    let conversation = Conversation {
        id: uuid::Uuid::new_v4().to_string(),
        title: title
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "新对话".into()),
        created_at: now,
        updated_at: now,
    };
    state
        .connection
        .lock()
        .map_err(|error| error.to_string())?
        .execute(
            "INSERT INTO conversations (id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                conversation.id,
                conversation.title,
                conversation.created_at,
                conversation.updated_at
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(conversation)
}

#[tauri::command]
pub fn list_conversations(state: tauri::State<Storage>) -> Result<Vec<Conversation>, String> {
    let connection = state.connection.lock().map_err(|error| error.to_string())?;
    let mut statement = connection
        .prepare(
            "SELECT id, title, created_at, updated_at FROM conversations ORDER BY updated_at DESC",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(Conversation {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

impl Storage {
    pub fn insert_message(
        &self,
        conversation_id: String,
        role: String,
        parts: serde_json::Value,
    ) -> Result<StoredMessage, String> {
        if !["user", "assistant"].contains(&role.as_str()) || !parts.is_array() {
            return Err("无效的消息内容".into());
        }
        let now = timestamp();
        let message = StoredMessage {
            id: uuid::Uuid::new_v4().to_string(),
            conversation_id,
            role,
            parts,
            created_at: now,
        };
        let serialized = serde_json::to_string(&message.parts).map_err(|error| error.to_string())?;
        let connection = self.connection.lock().map_err(|error| error.to_string())?;
        connection
            .execute(
                "INSERT INTO messages (id, conversation_id, role, parts, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![message.id, message.conversation_id, message.role, serialized, message.created_at],
            )
            .map_err(|error| error.to_string())?;
        connection
            .execute(
                "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
                params![now, message.conversation_id],
            )
            .map_err(|error| error.to_string())?;
        Ok(message)
    }

    pub fn messages(&self, conversation_id: &str) -> Result<Vec<StoredMessage>, String> {
        let connection = self.connection.lock().map_err(|error| error.to_string())?;
        let mut statement = connection
            .prepare("SELECT id, conversation_id, role, parts, created_at FROM messages WHERE conversation_id = ?1 ORDER BY created_at, rowid")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([conversation_id], |row| {
                let parts: String = row.get(3)?;
                Ok(StoredMessage {
                    id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    role: row.get(2)?,
                    parts: serde_json::from_str(&parts).unwrap_or(serde_json::Value::Array(vec![])),
                    created_at: row.get(4)?,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }
}

#[tauri::command]
pub fn append_message(
    conversation_id: String,
    role: String,
    parts: serde_json::Value,
    state: tauri::State<Storage>,
) -> Result<StoredMessage, String> {
    state.insert_message(conversation_id, role, parts)
}

#[tauri::command]
pub fn update_message(
    message_id: String,
    parts: serde_json::Value,
    state: tauri::State<Storage>,
) -> Result<(), String> {
    if !parts.is_array() {
        return Err("无效的消息内容".into());
    }
    let serialized = serde_json::to_string(&parts).map_err(|error| error.to_string())?;
    let now = timestamp();
    let connection = state.connection.lock().map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE messages SET parts = ?1 WHERE id = ?2",
            params![serialized, message_id],
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE conversations SET updated_at = ?1 WHERE id = (SELECT conversation_id FROM messages WHERE id = ?2)",
            params![now, message_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn load_messages(
    conversation_id: String,
    state: tauri::State<Storage>,
) -> Result<Vec<StoredMessage>, String> {
    state.messages(&conversation_id)
}

#[tauri::command]
pub fn delete_conversation(
    conversation_id: String,
    state: tauri::State<Storage>,
) -> Result<(), String> {
    state
        .connection
        .lock()
        .map_err(|error| error.to_string())?
        .execute("DELETE FROM conversations WHERE id = ?1", [conversation_id])
        .map_err(|error| error.to_string())?;
    Ok(())
}
