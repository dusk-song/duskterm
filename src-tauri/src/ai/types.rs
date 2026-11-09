use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    OpenaiChat,
    AnthropicMessages,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfig {
    pub id: String,
    pub model_id: String,
    pub name: String,
    pub enabled: bool,
    pub context_window: Option<u64>,
    #[serde(default)]
    pub reported_context_window: Option<u64>,
    #[serde(default)]
    pub reported_max_input_tokens: Option<u64>,
    pub max_output_tokens: u64,
    #[serde(default = "default_thinking_mode")]
    pub thinking_mode: String,
    #[serde(default = "default_thinking_format")]
    pub thinking_format: String,
    #[serde(default)]
    pub adapter_override: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub reasoning_budget: Option<u32>,
    #[serde(default)]
    pub thinking_on_params: Option<serde_json::Value>,
    #[serde(default)]
    pub thinking_off_params: Option<serde_json::Value>,
    #[serde(default)]
    pub capabilities: Capabilities,
    #[serde(default)]
    pub reported_capabilities: Capabilities,
    #[serde(default)]
    pub max_output_limit: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Capabilities {
    pub text: String,
    pub image: String,
    pub thinking: String,
    pub tools: String,
}
impl Default for Capabilities {
    fn default() -> Self {
        Self {
            text: "unknown".into(),
            image: "unknown".into(),
            thinking: "unknown".into(),
            tools: "unknown".into(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedModel {
    pub id: String,
    pub name: Option<String>,
    pub context_window: Option<u64>,
    pub max_input_tokens: Option<u64>,
    pub max_output_limit: Option<u64>,
    pub capabilities: Capabilities,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub protocol: Protocol,
    #[serde(default = "default_adapter")]
    pub adapter: String,
    #[serde(default)]
    pub has_key: bool,
    pub proxy_url: Option<String>,
    pub timeout_secs: u64,
    pub models: Vec<ModelConfig>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub services: Vec<Service>,
    pub default_model_id: Option<String>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    #[serde(default)]
    pub cached_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageStats {
    pub total_tokens: u64,
    pub cached_input_tokens: u64,
    // Only calls reporting both input and cached input enter this denominator.
    pub cache_input_tokens: u64,
    pub reported_calls: u64,
    pub unreported_calls: u64,
    pub cache_reported_calls: u64,
    pub partial_history: bool,
}
impl UsageStats {
    pub fn add(&mut self, usage: &Usage) {
        if let Some(total) = usage.total_tokens.or_else(|| {
            usage
                .input_tokens
                .zip(usage.output_tokens)
                .map(|(a, b)| a.saturating_add(b))
        }) {
            self.total_tokens = self.total_tokens.saturating_add(total);
            self.reported_calls += 1;
        } else {
            self.unreported_calls += 1;
        }
        if let Some((cached, input)) = usage
            .cached_input_tokens
            .zip(usage.input_tokens)
            .filter(|(cached, input)| cached <= input)
        {
            self.cached_input_tokens = self.cached_input_tokens.saturating_add(cached);
            self.cache_input_tokens = self.cache_input_tokens.saturating_add(input);
            self.cache_reported_calls += 1;
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub reasoning: String,
    pub status: String,
    pub model_id: Option<String>,
    pub service_name: Option<String>,
    pub actual_model_id: Option<String>,
    pub active: bool,
    pub reply_to: Option<String>,
    pub usage: Usage,
    pub elapsed_ms: u64,
    pub error: Option<String>,
    #[serde(default)]
    pub tools: Vec<ToolRecord>,
    // Canonical Rig call/result pairs, retained only for completed answers.
    #[serde(default)]
    pub tool_history: Vec<rig::message::Message>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<super::diagnosis::Task>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment: Option<super::deployment::Preview>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRecord {
    pub call_id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub target: String,
    pub root: String,
    pub status: String,
    pub started_at: u64,
    pub elapsed_ms: u64,
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_expires_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<Execution>,
}

pub(crate) use crate::ssh::exec::Execution;

pub(super) fn interrupt_tool(tool: &mut ToolRecord, reason: &str) {
    if !["running", "waiting_approval", "approved"].contains(&tool.status.as_str()) {
        return;
    }
    tool.approval_id = None;
    tool.approval_expires_at = None;
    tool.elapsed_ms = now().saturating_sub(tool.started_at);
    if let Some(execution) = &mut tool.execution {
        if ["dispatching", "running"].contains(&execution.outcome.as_str()) {
            execution.outcome = "unknown".into();
            execution.end_reason = "interrupted".into();
            execution.output_complete = false;
            tool.status = "unknown".into();
            let local = tool.name == super::tools::CMD_COMMAND
                || tool.arguments.pointer("/skill/targetKind").and_then(serde_json::Value::as_str) == Some("cmd");
            let outcome = if local {
                "本地命令结果未知，请先核实状态，不自动重试"
            } else {
                "已请求关闭执行通道，无法确认远端进程是否结束，请先核实状态，不自动重试"
            };
            tool.result = Some(format!("{reason}；{outcome}"));
        } else {
            tool.status = "interrupted".into();
            execution.outcome = "not_started".into();
            tool.result = Some(format!("{reason}；命令未派发，原确认已失效"));
        }
    } else {
        tool.status = "stopped".into();
        tool.result = Some(format!("{reason}；未获取可用查询结果"));
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub text: String,
    pub through: usize,
    pub revision: u64,
    pub model_id: String,
    pub usage: Usage,
    pub elapsed_ms: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub updated_at: u64,
    pub revision: u64,
    pub model_id: Option<String>,
    pub last_service_id: Option<String>,
    pub messages: Vec<Message>,
    pub summary: Option<Summary>,
    #[serde(default)]
    pub draft: String,
    #[serde(default)]
    pub usage_stats: Option<UsageStats>,
}
impl Conversation {
    pub fn ensure_usage_stats(&mut self) -> &mut UsageStats {
        if self.usage_stats.is_none() {
            // Older versions did not retain every summary's usage. Recover the
            // available records, and label them as partial instead of inventing it.
            let mut stats = UsageStats {
                partial_history: !self.messages.is_empty(),
                ..Default::default()
            };
            for message in self
                .messages
                .iter()
                .filter(|m| m.role == "assistant" && m.status != "streaming")
            {
                stats.add(&message.usage);
            }
            if let Some(summary) = &self.summary {
                stats.add(&summary.usage);
                stats.partial_history = true;
            }
            self.usage_stats = Some(stats);
        }
        self.usage_stats.as_mut().unwrap()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationInfo {
    pub id: String,
    pub title: String,
    pub updated_at: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendRequest {
    pub conversation_id: String,
    pub request_id: String,
    pub model_id: String,
    pub content: String,
    pub regenerate_id: Option<String>,
    pub confirmed_service_id: String,
    #[serde(default)]
    pub read_grant: Option<String>,
    #[serde(default)]
    pub exec_grant: Option<String>,
    #[serde(default)]
    pub diagnosis_target: Option<super::diagnosis::RequestTarget>,
    #[serde(default)]
    pub continue_diagnosis: Option<super::diagnosis::ContinueDiagnosis>,
    #[serde(default)]
    pub cmd_directory: Option<String>,
    #[serde(default)]
    pub analyze_message_id: Option<String>,
    #[serde(default)]
    pub deployment_target: Option<super::diagnosis::Target>,
    #[serde(default)]
    pub skill: Option<super::skill::Selection>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiEvent {
    pub conversation_id: String,
    pub request_id: String,
    pub message_id: String,
    pub seq: u64,
    pub kind: String,
    pub text: Option<String>,
    pub conversation: Option<Conversation>,
    pub tool: Option<ToolRecord>,
    pub task: Option<super::diagnosis::Task>,
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn new_message(role: &str, content: String) -> Message {
    Message {
        id: id(),
        role: role.into(),
        content,
        reasoning: String::new(),
        status: "completed".into(),
        model_id: None,
        service_name: None,
        actual_model_id: None,
        active: true,
        reply_to: None,
        usage: Usage::default(),
        elapsed_ms: 0,
        error: None,
        tools: Vec::new(),
        tool_history: Vec::new(),
        task: None,
        deployment: None,
    }
}

pub fn default_thinking_mode() -> String {
    "default".into()
}
pub fn default_thinking_format() -> String {
    String::new()
}
pub fn default_adapter() -> String {
    "generic".into()
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallRecord {
    pub id: String,
    pub started_at: u64,
    pub kind: String,
    pub service_name: String,
    pub model: Option<String>,
    pub protocol: Protocol,
    pub parameters: serde_json::Value,
    pub status: String,
    pub elapsed_ms: u64,
    pub usage: Usage,
    pub reasoning_chars: usize,
    pub error: Option<String>,
}
