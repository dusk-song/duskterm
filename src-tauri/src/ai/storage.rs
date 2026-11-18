use super::{diagnosis::{self, StepRef, TargetKind}, types::*};
use rusqlite::{params, Connection, OptionalExtension};
use std::{collections::HashSet, path::Path};

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
        if version > 8 {
            return Err("AI 数据库版本高于当前程序支持的版本".into());
        }
        if version < 7 {
            // Workspace support starts a fresh history for pre-release
            // databases. Model settings live in their own table.
            connection.execute_batch("BEGIN;
                CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL);
                DROP TABLE IF EXISTS generation_checkpoints;
                DROP TABLE IF EXISTS conversations;
                CREATE TABLE workspaces (id TEXT PRIMARY KEY, name TEXT NOT NULL, position INTEGER NOT NULL);
                INSERT INTO workspaces VALUES ('default', '默认工作区', 0);
                CREATE TABLE conversations (
                    id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL REFERENCES workspaces(id),
                    title TEXT NOT NULL, updated_at INTEGER NOT NULL, data TEXT NOT NULL, draft TEXT NOT NULL DEFAULT '');
                CREATE INDEX conversations_recent ON conversations(updated_at DESC);
                CREATE TABLE generation_checkpoints (
                    conversation_id TEXT PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
                    message_id TEXT NOT NULL, data TEXT NOT NULL);
                PRAGMA user_version=7; COMMIT;").map_err(db_error)?;
        }
        let db = Self { connection };
        if version < 8 {
            let transaction = db.connection.unchecked_transaction().map_err(db_error)?;
            transaction.execute_batch(
                "CREATE TABLE pending_target_changes (
                    conversation_id TEXT NOT NULL REFERENCES conversations(id),
                    message_id TEXT NOT NULL, call_id TEXT NOT NULL,
                    target_kind TEXT NOT NULL, target_identity TEXT NOT NULL,
                    PRIMARY KEY(conversation_id,message_id,call_id));
                CREATE INDEX pending_changes_by_target ON pending_target_changes(target_kind,target_identity);",
            ).map_err(db_error)?;
            if version == 7 {
                let ids: Vec<String> = transaction.prepare("SELECT id FROM conversations")
                    .map_err(db_error)?.query_map([], |row| row.get(0)).map_err(db_error)?
                    .collect::<Result<_, _>>().map_err(db_error)?;
                for id in ids {
                    for message in db.get(&id)?.messages {
                        Self::record_target_changes(&transaction, &id, &message)?;
                        Self::resolve_target_changes(&transaction, &id, &message)?;
                    }
                }
            }
            transaction.pragma_update(None, "user_version", 8).map_err(db_error)?;
            transaction.commit().map_err(db_error)?;
        }
        // Only unfinished generations need decoding and recovery at startup.
        let ids: Vec<String> = db
            .connection
            .prepare("SELECT conversation_id FROM generation_checkpoints")
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
                    m.tool_history.clear();
                    for tool in &mut m.tools {
                        super::types::interrupt_tool(tool, "应用退出导致操作中断");
                        if tool.execution.is_none() && tool.status == "stopped" {
                            tool.status = "interrupted".into();
                        }
                    }
                    m.status = "interrupted".into();
                    if let Some(task) = &mut m.task {
                        task.status = "interrupted".into();
                        if m.tools.iter().any(|tool| tool.status == "unknown") {
                            task.evidence_status = "unknown_remote_state".into();
                        }
                    }
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
                db.save_generation(&c)?;
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
        let Some(data) = data else {
            return Ok(Settings { default_ssh_approval_mode: Some(ApprovalMode::Manual), ..Settings::default() });
        };
        let mut settings: Settings = serde_json::from_str(&data).map_err(|_| "AI 配置数据损坏")?;
        if settings.migrate_ssh_approval() {
            self.save_settings(&settings)?;
        }
        Ok(settings)
    }
    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        let data = serde_json::to_string(settings).map_err(|_| "无法编码 AI 配置")?;
        self.connection.execute("INSERT INTO settings VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET data=excluded.data", [data]).map_err(db_error)?;
        Ok(())
    }
    pub fn history(&self) -> Result<ConversationHistory, String> {
        let workspaces = self.connection
            .prepare("SELECT id,name FROM workspaces ORDER BY position,id").map_err(db_error)?
            .query_map([], |row| Ok(AiWorkspace { id: row.get(0)?, name: row.get(1)? }))
            .map_err(db_error)?
            .collect::<Result<_, _>>().map_err(db_error)?;
        let conversations = self.connection
            .prepare("SELECT id,title,updated_at,workspace_id FROM conversations ORDER BY updated_at DESC,id")
            .map_err(db_error)?
            .query_map([], |r| {
                Ok(ConversationInfo {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    updated_at: r.get::<_, i64>(2)?.max(0) as u64,
                    workspace_id: r.get(3)?,
                })
            })
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error)?;
        Ok(ConversationHistory { workspaces, conversations })
    }

    pub fn create_workspace(&self, name: String) -> Result<AiWorkspace, String> {
        let workspace = AiWorkspace { id: id(), name };
        self.connection.execute(
            "INSERT INTO workspaces SELECT ?1,?2,MAX(position)+1 FROM workspaces",
            params![workspace.id,workspace.name],
        ).map_err(db_error)?;
        Ok(workspace)
    }

    pub fn rename_workspace(&self, id: &str, name: &str) -> Result<(), String> {
        let rows = self.connection.execute(
            "UPDATE workspaces SET name=?2 WHERE id=?1", params![id,name],
        ).map_err(db_error)?;
        (rows != 0).then_some(()).ok_or_else(|| "工作区不存在".into())
    }

    pub fn put_workspace_first(&self, id: &str) -> Result<(), String> {
        let rows = self.connection.execute(
            "UPDATE workspaces SET position=(SELECT MIN(position)-1 FROM workspaces) WHERE id=?1",
            [id],
        ).map_err(db_error)?;
        (rows != 0).then_some(()).ok_or_else(|| "工作区不存在".into())
    }

    pub fn move_conversation(&self, id: &str, workspace_id: &str) -> Result<(), String> {
        self.check_workspace(workspace_id)?;
        let rows = self.connection.execute(
            "UPDATE conversations SET workspace_id=?2 WHERE id=?1", params![id,workspace_id],
        ).map_err(db_error)?;
        (rows != 0).then_some(()).ok_or_else(|| "对话不存在".into())
    }

    fn check_workspace(&self, id: &str) -> Result<(), String> {
        if self.connection.query_row(
            "SELECT EXISTS (SELECT 1 FROM workspaces WHERE id=?1)", [id], |row| row.get::<_, bool>(0),
        ).map_err(db_error)? {
            Ok(())
        } else {
            Err("工作区不存在".into())
        }
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
        let checkpoint: Option<String> = self
            .connection
            .query_row(
                "SELECT data FROM generation_checkpoints WHERE conversation_id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_error)?;
        if let Some(data) = checkpoint {
            let message: Message = serde_json::from_str(&data).map_err(|_| "AI 生成检查点损坏")?;
            if let Some(last) = c
                .messages
                .last_mut()
                .filter(|m| m.id == message.id && m.status == "streaming")
            {
                *last = message;
            }
        }
        c.draft = draft;
        c.ensure_usage_stats();
        Ok(c)
    }
    pub fn save(&self, c: &Conversation) -> Result<(), String> {
        self.save_turn(c, None, false, &[])
    }

    pub fn save_generation(&self, c: &Conversation) -> Result<(), String> {
        self.save_turn(c, None, true, &[])
    }

    pub fn insert_conversation(&self, c: &Conversation, workspace_id: &str) -> Result<(), String> {
        if !c.messages.iter().any(|message| message.role == "user" && !message.content.trim().is_empty()) {
            return Err("发送首条消息后才保存对话".into());
        }
        self.check_workspace(workspace_id)?;
        self.save_turn(c, Some(workspace_id), true, &[])
    }

    pub fn verify_task_results(&self, id: &str, message_id: &str, steps: &[StepRef]) -> Result<Conversation, String> {
        let mut c = self.get(id)?;
        self.hydrate_unknown_steps(&mut c)?;
        diagnosis::verify_task_results(&mut c, message_id, steps)?;
        c.summary = None;
        c.revision += 1;
        self.save_turn(&c, None, false, steps)?;
        Ok(c)
    }

    fn save_turn(&self, c: &Conversation, workspace_id: Option<&str>, generation: bool, verified: &[StepRef]) -> Result<(), String> {
        let mut stored = c.clone();
        stored.draft.clear();
        let data = serde_json::to_string(&stored).map_err(|_| "无法编码 AI 对话")?;
        // Draft updates never get overwritten by a stream checkpoint.
        let transaction = self.connection.unchecked_transaction().map_err(db_error)?;
        if let Some(workspace_id) = workspace_id {
            transaction.execute(
                "INSERT INTO conversations(id,workspace_id,title,updated_at,data,draft) VALUES(?1,?2,?3,?4,?5,?6)",
                params![c.id,workspace_id,c.title,c.updated_at as i64,data,c.draft],
            ).map_err(db_error)?;
        } else {
            let rows = transaction.execute(
                "UPDATE conversations SET title=?2,updated_at=?3,data=?4 WHERE id=?1",
                params![c.id,c.title,c.updated_at as i64,data],
            ).map_err(db_error)?;
            if rows == 0 {
                return Err("对话不存在".into());
            }
        }
        let last = c.messages.last();
        if let Some(message) = last.filter(|_| generation) {
            Self::record_target_changes(&transaction, &c.id, message)?;
            Self::resolve_target_changes(&transaction, &c.id, message)?;
        }
        Self::delete_verified_steps(&transaction, &c.id, verified)?;
        if let Some(message) = last.filter(|m| m.status == "streaming") {
            let data = serde_json::to_string(message).map_err(|_| "无法编码生成检查点")?;
            transaction.execute("INSERT INTO generation_checkpoints VALUES(?1,?2,?3)
                ON CONFLICT(conversation_id) DO UPDATE SET message_id=excluded.message_id,data=excluded.data",
                params![c.id,message.id,data]).map_err(db_error)?;
        } else {
            transaction
                .execute(
                    "DELETE FROM generation_checkpoints WHERE conversation_id=?1",
                    [&c.id],
                )
                .map_err(db_error)?;
        }
        transaction.commit().map_err(db_error)?;
        Ok(())
    }
    pub fn checkpoint(&self, id: &str, message: &Message) -> Result<(), String> {
        let data = serde_json::to_string(message).map_err(|_| "无法编码生成检查点")?;
        let transaction = self.connection.unchecked_transaction().map_err(db_error)?;
        let rows = transaction.execute("UPDATE generation_checkpoints SET data=?1 WHERE conversation_id=?2 AND message_id=?3",
            params![data,id,message.id]).map_err(db_error)?;
        if rows == 0 {
            return Err("生成检查点已失效".into());
        }
        Self::record_target_changes(&transaction, id, message)?;
        transaction.commit().map_err(db_error)?;
        Ok(())
    }

    fn record_target_changes(connection: &Connection, id: &str, message: &Message) -> Result<(), String> {
        let Some(task) = message.task.as_ref().filter(|task| !task.analysis_only) else {
            return Ok(());
        };
        let Some(identity) = task.target_identity.as_deref() else { return Ok(()) };
        connection.execute(
            "DELETE FROM pending_target_changes WHERE conversation_id=?1 AND message_id=?2",
            params![id, message.id],
        ).map_err(db_error)?;
        for tool in &message.tools {
            let uncertain = tool.status == "unknown" || tool.execution.as_ref().is_some_and(|execution| {
                matches!(execution.outcome.as_str(), "dispatching" | "running") && !execution.has_exit_status()
            });
            if uncertain && !super::exec_access::verification_command(task.target_kind, &tool.arguments) {
                connection.execute("INSERT INTO pending_target_changes VALUES(?1,?2,?3,?4,?5)",
                    params![id, message.id, tool.call_id, task.target_kind.as_str(), identity],
                ).map_err(db_error)?;
            }
        }
        Ok(())
    }

    fn resolve_target_changes(connection: &Connection, id: &str, message: &Message) -> Result<(), String> {
        if let Some(task) = message.task.as_ref().filter(|task| !task.analysis_only) {
            Self::delete_verified_steps(connection, id, &task.verified_steps)?;
        }
        Ok(())
    }

    fn delete_verified_steps(connection: &Connection, id: &str, steps: &[StepRef]) -> Result<(), String> {
        for step in steps {
            connection.execute(
                "DELETE FROM pending_target_changes WHERE conversation_id=?1 AND message_id=?2 AND call_id=?3",
                params![id, step.message_id, step.call_id],
            ).map_err(db_error)?;
        }
        Ok(())
    }

    pub fn pending_target_change(
        &self, kind: TargetKind, identity: &str, conversation_id: &str, verified: &[StepRef],
    ) -> Result<Option<String>, String> {
        let mut statement = self.connection.prepare(
            "SELECT p.conversation_id,p.message_id,p.call_id,c.title FROM pending_target_changes p
             JOIN conversations c ON p.conversation_id=c.id
             WHERE p.target_kind=?1 AND p.target_identity=?2",
        ).map_err(db_error)?;
        let mut rows = statement.query(params![kind.as_str(),identity]).map_err(db_error)?;
        while let Some(row) = rows.next().map_err(db_error)? {
            let owner: String = row.get(0).map_err(db_error)?;
            let step = StepRef { message_id: row.get(1).map_err(db_error)?, call_id: row.get(2).map_err(db_error)? };
            if owner != conversation_id || !verified.contains(&step) {
                return row.get(3).map(Some).map_err(db_error);
            }
        }
        Ok(None)
    }

    fn pending_steps(&self, id: &str) -> Result<HashSet<StepRef>, String> {
        self.connection.prepare("SELECT message_id,call_id FROM pending_target_changes WHERE conversation_id=?1")
            .map_err(db_error)?
            .query_map([id], |row| Ok(StepRef { message_id: row.get(0)?, call_id: row.get(1)? }))
            .map_err(db_error)?.collect::<Result<_, _>>().map_err(db_error)
    }

    pub fn hydrate_unknown_steps(&self, c: &mut Conversation) -> Result<(), String> {
        diagnosis::hydrate_unknown_steps(c, &self.pending_steps(&c.id)?);
        Ok(())
    }

    pub fn check_delete_messages(&self, id: &str, message_ids: &[&str]) -> Result<(), String> {
        if self.pending_steps(id)?.iter().any(|step| message_ids.contains(&step.message_id.as_str())) {
            return Err("该消息有待核实的变更，请先在原消息核实结果".into());
        }
        Ok(())
    }
    pub fn draft_text(&self, id: &str) -> Result<String, String> {
        self.connection
            .query_row("SELECT draft FROM conversations WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .map_err(db_error)
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
        if !self.pending_steps(id)?.is_empty() {
            return Err("该对话有待核实的变更，请先在原消息核实结果".into());
        }
        self.connection
            .execute("DELETE FROM conversations WHERE id=?1", [id])
            .map_err(db_error)?;
        Ok(())
    }
}

fn db_error(_: rusqlite::Error) -> String {
    "AI 数据存储失败，请检查磁盘空间及文件权限".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn interrupted_terminal_chat() -> (Conversation, StepRef) {
        let mut chat = Conversation::empty("chat".into());
        let question = new_message("user", "更新 Compose 项目".into());
        let mut answer = new_message("assistant", String::new());
        answer.reply_to = Some(question.id.clone());
        answer.status = "failed".into();
        answer.active = false;
        let mut task = diagnosis::Task::new(
            "task",
            &question.content,
            diagnosis::Target { id: "ssh".into(), label: "测试目标".into() },
            TargetKind::Ssh,
        );
        task.status = "interrupted".into();
        task.directory = Some("/srv/project".into());
        task.target_identity = Some("host-a".into());
        answer.task = Some(task);
        answer.tools.push(ToolRecord {
            call_id: "step".into(), name: "execute_ssh_command".into(),
            arguments: json!({"command":"docker compose up -d"}), target: "ssh".into(),
            root: "/srv/project".into(), status: "unknown".into(), started_at: now(), elapsed_ms: 30,
            result: Some("SSH 已断开".into()), approval_id: None, approval_expires_at: None,
            approval_mode: Some(ApprovalMode::Manual),
            execution: Some(Execution { outcome: "unknown".into(), ..Default::default() }),
        });
        let step = StepRef { message_id: answer.id.clone(), call_id: "step".into() };
        chat.messages.extend([question, answer]);
        (chat, step)
    }

    #[test]
    fn terminal_result_verification_releases_only_confirmed_steps() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        let (mut chat, step) = interrupted_terminal_chat();
        chat.draft = "未发送的问题".into();
        db.insert_conversation(&chat, "default").unwrap();
        let mut other = chat.clone();
        other.id = "other".into();
        other.title = "另一个任务".into();
        db.insert_conversation(&other, "default").unwrap();

        let mut wrong_step = step.clone();
        wrong_step.call_id = "another-step".into();
        assert!(db.verify_task_results(&chat.id, &step.message_id, &[wrong_step]).is_err());
        assert!(db.check_delete_messages(&chat.id, &[step.message_id.as_str()]).is_err());
        let result = db.verify_task_results(&chat.id, &step.message_id, std::slice::from_ref(&step)).unwrap();
        assert_eq!(result.draft, chat.draft);
        assert_eq!(result.messages.len(), chat.messages.len());
        let message = result.messages.last().unwrap();
        assert_eq!(message.tools[0].status, "unknown");
        assert_eq!(message.task.as_ref().unwrap().evidence_status, "user_verified");
        assert!(message.task.as_ref().unwrap().unknown_steps.is_empty());
        assert!(db.check_delete_messages(&chat.id, &[step.message_id.as_str()]).is_ok());
        assert_eq!(db.pending_target_change(TargetKind::Ssh, "host-a", &chat.id, &[]).unwrap().as_deref(), Some("另一个任务"));
        assert!(db.verify_task_results(&chat.id, &step.message_id, std::slice::from_ref(&step)).is_err());
        assert!(db.check_delete_messages(&other.id, &[step.message_id.as_str()]).is_err());
    }

    #[test]
    fn terminal_follow_up_keeps_interrupted_evidence_and_reports_target_match() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        let (chat, _) = interrupted_terminal_chat();
        db.insert_conversation(&chat, "default").unwrap();
        let mut chat = db.get(&chat.id).unwrap();
        db.hydrate_unknown_steps(&mut chat).unwrap();
        let next = new_message("user", "当时发生了什么".into());
        let id = next.id.clone();
        chat.messages.push(next);
        let same = diagnosis::follow_up_context(&chat, &id, Some((TargetKind::Ssh, "host-a")), 8192).unwrap();
        assert!(same.contains("本轮绑定与记录匹配的终端"));
        assert!(same.contains("/srv/project") && same.contains("SSH 已断开") && same.contains(r#""pending":1"#));
        let different = diagnosis::follow_up_context(&chat, &id, Some((TargetKind::Ssh, "host-b")), 8192).unwrap();
        assert!(different.contains("无法确认匹配"));
        let offline = diagnosis::follow_up_context(&chat, &id, None, 8192).unwrap();
        assert!(offline.contains("SSH 已断开"));
        assert!(diagnosis::follow_up_context(&chat, &chat.messages[0].id, None, 8192).is_none());
    }

    #[test]
    fn ssh_approval_migration_persists_one_conservative_default() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        assert_eq!(db.settings().unwrap().default_ssh_approval_mode, Some(ApprovalMode::Manual));
        let legacy_model = |id, enabled, automatic: Option<bool>| {
            let mut model = json!({"id":id,"modelId":id,"name":id,"enabled":enabled,"maxOutputTokens":1024});
            if let Some(automatic) = automatic {
                model["autoExecuteCommands"] = json!(automatic);
            }
            model
        };
        for (models, expected) in [
            (vec![legacy_model("a", true, None), legacy_model("b", false, Some(false))], ApprovalMode::Automatic),
            (vec![legacy_model("a", true, Some(true)), legacy_model("b", true, Some(false))], ApprovalMode::Manual),
            (vec![legacy_model("a", false, Some(true))], ApprovalMode::Manual),
        ] {
            let data = json!({"defaultModelId":"a","services":[{
                "id":"service","name":"service","baseUrl":"https://example.invalid",
                "protocol":"openai_chat","timeoutSecs":60,"models":models
            }]});
            db.connection.execute("INSERT OR REPLACE INTO settings VALUES (1,?1)", [data.to_string()]).unwrap();
            let migrated = db.settings().unwrap();
            assert_eq!(migrated.default_ssh_approval_mode, Some(expected));
            let saved: String = db.connection.query_row("SELECT data FROM settings", [], |row| row.get(0)).unwrap();
            assert_eq!(serde_json::from_str::<Settings>(&saved).unwrap().default_ssh_approval_mode, Some(expected));
        }
        let mut settings = db.settings().unwrap();
        settings.services[0].models[0].enabled = true;
        db.save_settings(&settings).unwrap();
        assert_eq!(db.settings().unwrap().default_ssh_approval_mode, Some(ApprovalMode::Manual));
    }

    #[test]
    fn ssh_approval_stays_with_conversation_and_resets_when_switching_targets() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        let mut chat = Conversation::empty("chat".into());
        chat.messages.push(new_message("user", "检查 Vim".into()));
        db.insert_conversation(&chat, "default").unwrap();
        let automatic = ApprovalMode::Automatic;
        let manual = ApprovalMode::Manual;
        let first = SshApproval::bind(chat.ssh_approval.as_ref(), "host-a".into(), automatic);
        assert_eq!(first.mode, automatic);
        chat.ssh_approval = Some(first);
        db.save(&chat).unwrap();
        let mut chat = db.get(&chat.id).unwrap();
        chat.model_id = Some("other-model".into());
        let reconnected = SshApproval::bind(chat.ssh_approval.as_ref(), "host-a".into(), manual);
        assert_eq!(reconnected.mode, automatic);
        let switched = SshApproval::bind(Some(&reconnected), "host-b".into(), automatic);
        assert_eq!(switched.mode, manual);
        let switched_back = SshApproval::bind(Some(&switched), "host-a".into(), automatic);
        assert_eq!(switched_back.mode, manual);
    }
}
