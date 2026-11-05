use super::types::*;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

// Independent schema version: command history owns the version of duskterm.db.
pub struct Database {
    connection: Connection,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| "无法创建 AI 数据目录")?;
        }
        let connection = Connection::open(path).map_err(|_| "无法打开 AI 数据库")?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(db_error)?;
        connection
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(db_error)?;
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(db_error)?;
        if version > 2 {
            return Err("AI 数据库版本高于当前程序支持的版本".into());
        }
        if version == 0 {
            connection.execute_batch("BEGIN;
                CREATE TABLE settings (id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL);
                CREATE TABLE conversations (id TEXT PRIMARY KEY, title TEXT NOT NULL, updated_at INTEGER NOT NULL, data TEXT NOT NULL, draft TEXT NOT NULL DEFAULT '');
                CREATE INDEX conversations_recent ON conversations(updated_at DESC);
                PRAGMA user_version=1; COMMIT;").map_err(db_error)?;
        }
        let db = Self { connection };
        // A terminated desktop process cannot leave an answer marked as complete.
        let ids: Vec<String> = db
            .connection
            .prepare("SELECT id FROM conversations")
            .map_err(db_error)?
            .query_map([], |r| r.get(0))
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error)?;
        for id in ids {
            let mut c = db.get(&id)?;
            let mut changed = false;
            for m in &mut c.messages {
                if m.status == "streaming" {
                    m.status = "interrupted".into();
                    m.error = Some("上次生成因应用退出而中断".into());
                    changed = true;
                }
            }
            if changed {
                // Restore a previously completed version if a regeneration was
                // interrupted before its replacement became usable.
                let users: Vec<String> = c
                    .messages
                    .iter()
                    .filter(|m| m.role == "user")
                    .map(|m| m.id.clone())
                    .collect();
                for user in users {
                    if !c.messages.iter().any(|m| {
                        m.reply_to.as_ref() == Some(&user) && m.status == "completed" && m.active
                    }) {
                        if let Some(previous) =
                            c.messages.iter_mut().rev().find(|m| {
                                m.reply_to.as_ref() == Some(&user) && m.status == "completed"
                            })
                        {
                            previous.active = true;
                        }
                    }
                }
                db.save(&c)?;
            }
        }
        Ok(db)
    }

    pub fn settings(&self) -> Result<Settings, String> {
        let data: Option<String> = self
            .connection
            .query_row("SELECT data FROM settings WHERE id=1", [], |r| r.get(0))
            .optional()
            .map_err(db_error)?;
        data.map(|s| serde_json::from_str(&s).map_err(|_| "AI 配置数据损坏".into()))
            .unwrap_or(Ok(Settings::default()))
    }
    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        let data = serde_json::to_string(settings).map_err(|_| "无法编码 AI 配置")?;
        self.connection.execute("INSERT INTO settings VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET data=excluded.data", [data]).map_err(db_error)?;
        Ok(())
    }
    pub fn list(&self) -> Result<Vec<ConversationInfo>, String> {
        self.connection
            .prepare("SELECT id,title,updated_at FROM conversations ORDER BY updated_at DESC")
            .map_err(db_error)?
            .query_map([], |r| {
                Ok(ConversationInfo {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    updated_at: r.get::<_, i64>(2)?.max(0) as u64,
                })
            })
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error)
    }
    pub fn get(&self, id: &str) -> Result<Conversation, String> {
        let (data, draft): (String, String) = self
            .connection
            .query_row(
                "SELECT data,draft FROM conversations WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| "对话不存在或无法读取")?;
        let mut c: Conversation = serde_json::from_str(&data).map_err(|_| "AI 对话数据损坏")?;
        c.draft = draft;
        c.ensure_usage_stats();
        Ok(c)
    }
    pub fn save(&self, c: &Conversation) -> Result<(), String> {
        let mut stored = c.clone();
        stored.draft.clear();
        let data = serde_json::to_string(&stored).map_err(|_| "无法编码 AI 对话")?;
        // Draft updates never get overwritten by a stream checkpoint.
        self.connection.execute("INSERT INTO conversations(id,title,updated_at,data,draft) VALUES(?1,?2,?3,?4,?5)
            ON CONFLICT(id) DO UPDATE SET title=excluded.title,updated_at=excluded.updated_at,data=excluded.data",
            params![c.id,c.title,c.updated_at as i64,data,c.draft]).map_err(db_error)?;
        Ok(())
    }
    pub fn draft(&self, id: &str, text: &str) -> Result<(), String> {
        if text.len() > 256_000 {
            return Err("草稿过长".into());
        }
        self.connection
            .execute(
                "UPDATE conversations SET draft=?1 WHERE id=?2",
                params![text, id],
            )
            .map_err(db_error)?;
        Ok(())
    }
    pub fn delete(&self, id: &str) -> Result<(), String> {
        self.connection
            .execute("DELETE FROM conversations WHERE id=?1", [id])
            .map_err(db_error)?;
        Ok(())
    }
}

fn db_error(_: rusqlite::Error) -> String {
    "AI 数据存储失败，请检查磁盘空间及文件权限".into()
}
