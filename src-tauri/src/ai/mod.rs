mod call_log;
mod compatibility;
mod provider;
mod runtime;
mod storage;
mod types;

use std::sync::{Arc, Mutex};
use tauri::{ipc::Channel, State};
use tokio::sync::watch;
use types::*;

#[derive(Clone)]
pub struct AiState {
    db: Arc<Mutex<Result<storage::Database, String>>>,
    active: Arc<Mutex<Option<Active>>>,
    call_log: Arc<call_log::CallLog>,
}
struct Active {
    request_id: String,
    conversation_id: String,
    cancel: watch::Sender<bool>,
}

impl AiState {
    pub fn new(path: &std::path::Path) -> Self {
        Self {
            db: Arc::new(Mutex::new(storage::Database::open(path))),
            active: Arc::new(Mutex::new(None)),
            call_log: Arc::new(call_log::CallLog::new(path)),
        }
    }
    fn with_db<T>(
        &self,
        f: impl FnOnce(&storage::Database) -> Result<T, String>,
    ) -> Result<T, String> {
        let db = self.db.lock().map_err(|_| "AI 数据库状态不可用")?;
        match &*db {
            Ok(db) => f(db),
            Err(error) => Err(error.clone()),
        }
    }
    fn start_call(&self, kind: &str, service: &Service, model: Option<&ModelConfig>) -> CallRecord {
        let parameters = model.map_or_else(
            || serde_json::json!({"adapter":service.adapter}),
            |m| compatibility::log_parameters(m, service),
        );
        let record = CallRecord {
            id: id(),
            started_at: now(),
            kind: kind.into(),
            service_name: service.name.clone(),
            model: model.map(|m| m.model_id.clone()),
            protocol: service.protocol.clone(),
            parameters,
            status: "running".into(),
            elapsed_ms: 0,
            usage: Usage::default(),
            reasoning_chars: 0,
            error: None,
        };
        self.call_log.write(&record);
        record
    }
    fn finish_call(
        &self,
        record: &mut CallRecord,
        status: &str,
        error: Option<String>,
        usage: Usage,
        reasoning_chars: usize,
    ) {
        record.status = status.into();
        record.error = error;
        record.usage = usage;
        record.reasoning_chars = reasoning_chars;
        record.elapsed_ms = now().saturating_sub(record.started_at);
        self.call_log.write(record);
    }

    pub fn cancel_all(&self) {
        if let Ok(active) = self.active.lock() {
            if let Some(active) = &*active {
                let _ = active.cancel.send(true);
            }
        }
    }
    fn idle_for(
        &self,
        conversation_id: &str,
    ) -> Result<std::sync::MutexGuard<'_, Option<Active>>, String> {
        let active = self.active.lock().map_err(|_| "AI 请求状态不可用")?;
        if active
            .as_ref()
            .is_some_and(|a| a.conversation_id == conversation_id)
        {
            return Err("请先停止当前对话的生成，再修改消息".into());
        }
        Ok(active)
    }
}

fn enabled_model(settings: &Settings, id: &str) -> Result<(Service, ModelConfig), String> {
    for service in &settings.services {
        if let Some(model) = service.models.iter().find(|m| m.id == id && m.enabled) {
            return Ok((service.clone(), model.clone()));
        }
    }
    Err("所选模型不存在或已禁用，请重新选择模型".into())
}

#[tauri::command]
pub async fn ai_get_settings(state: State<'_, AiState>) -> Result<Settings, String> {
    state.with_db(|db| db.settings())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogQuery {
    id: String,
    model_id: String,
    adapter: String,
}

#[tauri::command]
pub fn ai_model_catalog(models: Vec<CatalogQuery>) -> Result<serde_json::Value, String> {
    if models.len() > 2000 {
        return Err("模型数量过多".into());
    }
    let mut result = serde_json::Map::new();
    for query in models {
        if let Some(spec) = compatibility::model_spec(&query.adapter, &query.model_id) {
            let state = |flag| if flag { "supported" } else { "unsupported" };
            result.insert(query.id,serde_json::json!({"capabilities":{"text":state(spec.input.text),"image":state(spec.input.image),"thinking":state(spec.reasoning.supported())},
                "efforts":spec.reasoning.levels().unwrap_or_default().iter().map(|e|e.as_str()).collect::<Vec<_>>(),
                "budgetMin":spec.reasoning.budget().map(|b|*b.start()),"budgetMax":spec.reasoning.budget().map(|b|*b.end()),
                "canDisable":spec.reasoning.can_disable(),"maxInputTokens":spec.input_limit(),"contextWindow":spec.context_window,"maxOutputTokens":spec.max_output_tokens}));
        }
    }
    Ok(serde_json::Value::Object(result))
}

#[tauri::command]
pub async fn ai_save_service(
    state: State<'_, AiState>,
    mut service: Service,
    api_key: Option<String>,
    default_model_id: Option<String>,
) -> Result<Settings, String> {
    provider::validate(&mut service)?;
    if default_model_id
        .as_ref()
        .is_some_and(|id| !service.models.iter().any(|m| &m.id == id && m.enabled))
    {
        return Err("默认模型须为当前服务中已启用的模型".into());
    }
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let key = api_key.map(zeroize::Zeroizing::new);
        if key
            .as_ref()
            .is_some_and(|k| k.len() > 8192 || k.contains(['\r', '\n']))
        {
            return Err("API Key 格式无效".into());
        }
        state.with_db(|db| {
            let mut settings = db.settings()?;
            let previous = settings
                .services
                .iter()
                .find(|s| s.id == service.id)
                .cloned();
            let has_new = key.as_ref().is_some_and(|k| !k.trim().is_empty());
            if previous.as_ref().is_some_and(|old| {
                old.has_key
                    && (old.base_url != service.base_url || old.protocol != service.protocol)
            }) && !has_new
            {
                return Err("地址或协议已变更，请重新输入用于此目标的 API Key。".into());
            }
            if settings
                .services
                .iter()
                .filter(|s| s.id != service.id)
                .flat_map(|s| &s.models)
                .any(|m| service.models.iter().any(|n| n.id == m.id))
            {
                return Err("模型标识跨服务重复".into());
            }
            if previous.is_none() && settings.services.len() >= 100 {
                return Err("最多配置 100 个服务实例".into());
            }
            let old_key = if let Some(old) = &previous {
                if old.has_key && has_new {
                    provider::stored_credential(&old.id)?
                } else {
                    None
                }
            } else {
                None
            };
            service.has_key = previous.as_ref().is_some_and(|s| s.has_key);
            if has_new {
                provider::entry(&service.id)?
                    .set_password(key.as_ref().unwrap().trim())
                    .map_err(|_| "无法保存 API Key，请启用并解锁系统凭据库")?;
                service.has_key = true;
            }
            if let Some(existing) = settings.services.iter_mut().find(|s| s.id == service.id) {
                *existing = service.clone();
            } else {
                settings.services.push(service.clone());
            }
            if let Some(id) = default_model_id {
                enabled_model(&settings, &id)?;
                settings.default_model_id = Some(id);
            }
            if settings
                .default_model_id
                .as_ref()
                .is_some_and(|id| enabled_model(&settings, id).is_err())
            {
                settings.default_model_id = None;
            }
            if let Err(error) = db.save_settings(&settings) {
                if has_new {
                    if let Some(old) = old_key {
                        let _ = provider::entry(&service.id)?.set_password(&old);
                    } else {
                        let _ = provider::entry(&service.id)?.delete_credential();
                    }
                }
                return Err(error);
            }
            Ok(settings)
        })
    })
    .await
    .map_err(|_| "服务配置任务中断")?
}

#[tauri::command]
pub async fn ai_delete_service(state: State<'_, AiState>, id: String) -> Result<Settings, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state.with_db(|db| {
            let mut settings = db.settings()?;
            let service = settings
                .services
                .iter()
                .find(|s| s.id == id)
                .ok_or("服务不存在")?;
            let old_key = if service.has_key {
                provider::stored_credential(&service.id)?
            } else {
                None
            };
            if service.has_key {
                match provider::entry(&id)?.delete_credential() {
                    Ok(()) | Err(keyring::Error::NoEntry) => {}
                    Err(_) => return Err("无法删除系统凭据，请重试".into()),
                }
            }
            settings.services.retain(|s| s.id != id);
            if settings
                .default_model_id
                .as_ref()
                .is_some_and(|id| enabled_model(&settings, id).is_err())
            {
                settings.default_model_id = None;
            }
            if let Err(error) = db.save_settings(&settings) {
                if let Some(old) = old_key {
                    let _ = provider::entry(&id)?.set_password(&old);
                }
                return Err(error);
            }
            Ok(settings)
        })
    })
    .await
    .map_err(|_| "服务删除任务中断")?
}

#[tauri::command]
pub async fn ai_set_default_model(
    state: State<'_, AiState>,
    model_id: Option<String>,
) -> Result<Settings, String> {
    state.with_db(|db| {
        let mut settings = db.settings()?;
        if let Some(id) = &model_id {
            enabled_model(&settings, id)?;
        }
        settings.default_model_id = model_id;
        db.save_settings(&settings)?;
        Ok(settings)
    })
}

fn probe_service(
    settings: &Settings,
    service_id: Option<&str>,
    draft: Option<Service>,
    kind: &str,
    model_id: Option<&str>,
) -> Result<Service, String> {
    let mut service = draft
        .or_else(|| {
            settings
                .services
                .iter()
                .find(|s| Some(s.id.as_str()) == service_id)
                .cloned()
        })
        .ok_or("服务不存在")?;
    match kind {
        "models" | "credentials" => service.models.clear(),
        "model" => {
            service.models.retain(|m| Some(m.id.as_str()) == model_id);
            if service.models.is_empty() {
                return Err("请选择需要测试的模型".into());
            }
        }
        _ => return Err("未知的测试类型".into()),
    }
    if service.name.trim().is_empty() {
        service.name = "未命名服务".into();
    }
    provider::validate(&mut service)?;
    Ok(service)
}

fn probe_key(
    settings: &Settings,
    service: &Service,
    api_key: Option<String>,
) -> Result<zeroize::Zeroizing<String>, String> {
    if let Some(key) = api_key
        .map(zeroize::Zeroizing::new)
        .filter(|k| !k.trim().is_empty())
    {
        if key.len() > 8192 || key.contains(['\r', '\n']) {
            return Err("API Key 格式无效".into());
        }
        return Ok(zeroize::Zeroizing::new(key.trim().to_string()));
    }
    if let Some(old) = settings
        .services
        .iter()
        .find(|s| s.id == service.id && s.has_key)
    {
        if old.base_url != service.base_url || old.protocol != service.protocol {
            return Err("地址或协议已变更，请输入用于此目标的 API Key 后测试".into());
        }
        return provider::credential(old);
    }
    Ok(zeroize::Zeroizing::new(String::new()))
}

#[tauri::command]
pub async fn ai_probe(
    state: State<'_, AiState>,
    service_id: Option<String>,
    draft: Option<Service>,
    api_key: Option<String>,
    kind: String,
    model_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let settings = state.with_db(|db| db.settings())?;
    let service = probe_service(
        &settings,
        service_id.as_deref(),
        draft,
        &kind,
        model_id.as_deref(),
    )?;
    let key_service = service.clone();
    let key =
        tauri::async_runtime::spawn_blocking(move || probe_key(&settings, &key_service, api_key))
            .await
            .map_err(|_| "凭据读取任务中断")??;
    let selected_model = service
        .models
        .iter()
        .find(|m| Some(&m.id) == model_id.as_ref());
    let mut record = state.start_call(&kind, &service, selected_model);
    let started = std::time::Instant::now();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(service.timeout_secs),
        async {
            match kind.as_str() {
                "models" => Ok::<serde_json::Value, String>(
                    serde_json::json!({"models":provider::list(&service,&key).await?}),
                ),
                "credentials" => {
                    provider::verify(&service, &key).await?;
                    Ok(serde_json::json!({"ok":true}))
                }
                "model" => {
                    let model = service
                        .models
                        .iter()
                        .find(|m| Some(&m.id) == model_id.as_ref())
                        .ok_or("请选择需要测试的模型")?;
                    let (usage, reasoning_chars) = provider::test(&service, model, &key).await?;
                    Ok(serde_json::json!({"usage":usage,"reasoningChars":reasoning_chars}))
                }
                _ => Err("未知的测试类型".into()),
            }
        },
    )
    .await
    .map_err(|_| "TIMEOUT: 测试超时".to_string())
    .and_then(|result| result);
    let status = if result.is_ok() {
        "completed"
    } else {
        "failed"
    };
    let usage = result
        .as_ref()
        .ok()
        .and_then(|r| r.get("usage"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let reasoning_chars = result
        .as_ref()
        .ok()
        .and_then(|r| r.get("reasoningChars"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    state.finish_call(
        &mut record,
        status,
        result.as_ref().err().cloned(),
        usage,
        reasoning_chars,
    );
    let result = result?;
    Ok(serde_json::json!({"result":result,"elapsedMs":started.elapsed().as_millis() as u64}))
}

#[tauri::command]
pub async fn ai_list_conversations(
    state: State<'_, AiState>,
) -> Result<Vec<ConversationInfo>, String> {
    state.with_db(|db| db.list())
}
#[tauri::command]
pub async fn ai_get_conversation(
    state: State<'_, AiState>,
    id: String,
) -> Result<Conversation, String> {
    state.with_db(|db| db.get(&id))
}
#[tauri::command]
pub async fn ai_create_conversation(state: State<'_, AiState>) -> Result<Conversation, String> {
    let c = Conversation {
        id: id(),
        title: "新对话".into(),
        updated_at: now(),
        revision: 0,
        model_id: None,
        last_service_id: None,
        messages: vec![],
        summary: None,
        draft: String::new(),
        usage_stats: Some(UsageStats::default()),
    };
    state.with_db(|db| db.save(&c))?;
    Ok(c)
}
#[tauri::command]
pub async fn ai_save_draft(
    state: State<'_, AiState>,
    id: String,
    text: String,
) -> Result<(), String> {
    state.with_db(|db| db.draft(&id, &text))
}
#[tauri::command]
pub async fn ai_delete_conversation(state: State<'_, AiState>, id: String) -> Result<(), String> {
    let _idle = state.idle_for(&id)?;
    state.with_db(|db| db.delete(&id))
}

#[tauri::command]
pub async fn ai_delete_message(
    state: State<'_, AiState>,
    conversation_id: String,
    message_id: String,
) -> Result<Conversation, String> {
    let _idle = state.idle_for(&conversation_id)?;
    state.with_db(|db| {
        let mut c = db.get(&conversation_id)?;
        let target = c
            .messages
            .iter()
            .find(|m| m.id == message_id)
            .ok_or("消息不存在")?;
        let is_user = target.role == "user";
        c.messages.retain(|m| {
            m.id != message_id && !(is_user && m.reply_to.as_ref() == Some(&message_id))
        });
        c.summary = None;
        if is_user {
            c.title = c
                .messages
                .iter()
                .find(|m| m.role == "user")
                .map_or_else(|| "新对话".into(), |m| m.content.chars().take(32).collect());
        }
        c.revision += 1;
        c.updated_at = now();
        db.save(&c)?;
        Ok(c)
    })
}

#[tauri::command]
pub async fn ai_select_answer(
    state: State<'_, AiState>,
    conversation_id: String,
    message_id: String,
) -> Result<Conversation, String> {
    let _idle = state.idle_for(&conversation_id)?;
    state.with_db(|db| {
        let mut c = db.get(&conversation_id)?;
        let target = c
            .messages
            .iter()
            .find(|m| m.id == message_id && m.role == "assistant" && m.status == "completed")
            .ok_or("只能选择完整回答")?;
        let reply_to = target.reply_to.clone();
        for m in &mut c.messages {
            if m.role == "assistant" && m.reply_to == reply_to {
                m.active = m.id == message_id;
            }
        }
        c.summary = None;
        c.revision += 1;
        db.save(&c)?;
        Ok(c)
    })
}

#[tauri::command]
pub async fn ai_cancel(state: State<'_, AiState>, request_id: String) -> Result<(), String> {
    let active = state.active.lock().map_err(|_| "AI 请求状态不可用")?;
    if let Some(active) = &*active {
        if active.request_id == request_id {
            let _ = active.cancel.send(true);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn ai_send(
    state: State<'_, AiState>,
    request: SendRequest,
    channel: Channel<AiEvent>,
) -> Result<Conversation, String> {
    runtime::send(state.inner().clone(), request, channel).await
}
