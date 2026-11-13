//! Durable diagnosis identity, run metadata and evidence. Rig drives each run.
use super::{types::*, AiState};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub(super) const MODEL_LIMIT: usize = 8;
pub(super) const TOOL_LIMIT: usize = 12;
pub(super) const TASK_TIMEOUT: u64 = 300;
pub(super) const SUMMARY_TIME: u64 = 20;
pub(super) const WRAP_UP: &str = "TASK_WRAP_UP";
const COMMON: &str = include_str!("prompts/diagnosis.md");
const SSH: &str = include_str!("prompts/ssh.md");
const CMD: &str = include_str!("prompts/cmd.md");
pub(super) const SUMMARY: &str = include_str!("prompts/evidence-summary.md");
pub(super) const SUMMARY_PROTOCOL_ERROR: &str =
    "模型返回了异常的工具调用标记，证据整理未完成。已有工具结果可查看。";

pub(super) fn has_raw_tool_markup(text: &str) -> bool {
    ["<｜｜DSML｜｜", "</｜｜DSML｜｜", "<||DSML||", "</||DSML||"]
        .iter()
        .any(|marker| text.contains(marker))
}

pub(super) fn preamble(kind: TargetKind) -> String {
    let shell = match kind {
        TargetKind::Ssh => SSH,
        TargetKind::Cmd => CMD,
    };
    format!("{COMMON}\n{shell}")
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestTarget {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub kind: TargetKind,
    #[serde(default)]
    pub terminal_task: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TargetKind {
    #[default]
    Ssh,
    Cmd,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContinueDiagnosis {
    pub message_id: String,
    pub rebind_session_id: Option<String>,
    #[serde(default)]
    pub verified_steps: Vec<StepRef>,
}

#[derive(Clone, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepRef {
    pub message_id: String,
    pub call_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnknownStep {
    #[serde(flatten)]
    pub step: StepRef,
    pub run: usize,
    pub index: usize,
    pub command: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub goal: String,
    pub target: Target,
    #[serde(default)]
    pub target_kind: TargetKind,
    #[serde(default)]
    pub terminal_task: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub automatic: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<super::skill::Info>,
    pub status: String,
    pub analysis_only: bool,
    pub parent_message_id: Option<String>,
    #[serde(default)]
    pub run_number: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_identity: Option<String>,
    pub started_at: u64,
    pub elapsed_ms: u64,
    pub model_calls: usize,
    pub tool_calls: usize,
    pub model_limit: usize,
    pub tool_limit: usize,
    pub command_limit: usize,
    pub evidence_status: String,
    pub context_snapshot: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verified_steps: Vec<StepRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown_steps: Vec<UnknownStep>,
}
impl Task {
    pub fn new(request_id: &str, goal: &str, target: Target, target_kind: TargetKind) -> Self {
        Self {
            id: request_id.into(),
            goal: goal.chars().take(4096).collect(),
            target,
            target_kind,
            terminal_task: false,
            automatic: false,
            directory: None,
            skill: None,
            status: "running".into(),
            analysis_only: false,
            parent_message_id: None,
            run_number: 1,
            target_identity: None,
            started_at: now(),
            elapsed_ms: 0,
            model_calls: 0,
            tool_calls: 0,
            model_limit: MODEL_LIMIT,
            tool_limit: TOOL_LIMIT,
            command_limit: TOOL_LIMIT,
            evidence_status: "insufficient_evidence".into(),
            context_snapshot: None,
            verified_steps: Vec::new(),
            unknown_steps: Vec::new(),
        }
    }
    pub fn finish(
        &mut self,
        message_status: &str,
        error: Option<&str>,
        tools: &[ToolRecord],
        wrapped_up: bool,
    ) {
        self.elapsed_ms = now().saturating_sub(self.started_at);
        self.status = if error.is_some_and(|error| error.contains("BUDGET:")) {
            "budget_exhausted"
        } else if error.is_some_and(|error| error.starts_with("TIMEOUT:")) {
            "timed_out"
        } else if wrapped_up && message_status == "completed" && self.skill.is_none() {
            "paused"
        } else if wrapped_up && message_status == "failed" {
            "summary_failed"
        } else {
            message_status
        }
        .into();
        if !self.unknown_steps.is_empty() || tools.iter().any(|tool| tool.status == "unknown") {
            self.evidence_status = "unknown_remote_state".into();
        } else if self.evidence_status != "unknown_remote_state"
            && tools.iter().any(|tool| {
                (tool.status == "completed" && super::tools::is_observation(&tool.name)) || tool.execution
                    .as_ref()
                    .is_some_and(|execution| execution.outcome == "exited")
            })
        {
            self.evidence_status = "evidence_available".into();
        }
    }
}

fn continuation_source<'a>(
    c: &'a Conversation,
    intent: &ContinueDiagnosis,
) -> Result<(&'a Message, &'a Task), String> {
    let original = c
        .messages
        .iter()
        .find(|message| message.id == intent.message_id && message.role == "assistant")
        .and_then(|message| message.task.as_ref())
        .filter(|task| !task.analysis_only)
        .ok_or("找不到可继续检查的诊断任务")?;
    c.messages
        .iter()
        .rev()
        .find_map(|message| {
            let task = message.task.as_ref()?;
            (task.id == original.id && !task.analysis_only && message.status != "streaming")
                .then_some((message, task))
        })
        .ok_or_else(|| "原诊断任务尚未结束".into())
}

pub(super) fn continue_task(
    c: &Conversation,
    request: &SendRequest,
    target: &RequestTarget,
    identity: &str,
) -> Result<Task, String> {
    let mut task = Task::new(
        &request.request_id,
        request.content.trim(),
        Target {
            id: target.id.clone(),
            label: target.label.clone(),
        },
        target.kind,
    );
    task.target_identity = Some(identity.into());
    task.terminal_task = target.terminal_task;
    let Some(intent) = &request.continue_diagnosis else {
        return Ok(task);
    };
    let (parent, old) = continuation_source(c, intent)?;
    if old.skill.is_some() {
        return Err("固定脚本任务可继续分析。要再次运行请重新选择 Skill 并确认。".into());
    }
    let session_id = intent
        .rebind_session_id
        .as_deref()
        .unwrap_or(&old.target.id);
    if task.target.id != session_id || task.target_kind != old.target_kind || task.terminal_task != old.terminal_task {
        return Err("续查目标已变化，请重新选择原诊断目标".into());
    }
    if old
        .target_identity
        .as_deref()
        .is_some_and(|saved| saved != identity)
    {
        return Err("当前连接与原诊断目标不匹配，请打开原 SSH 连接或相同启动目录的 CMD".into());
    }
    task.id = old.id.clone();
    task.goal = old.goal.clone();
    task.parent_message_id = Some(parent.id.clone());
    task.run_number = old.run_number.max(1) + 1;
    task.directory = old.directory.clone();
    task.automatic = old.automatic;
    task.unknown_steps = unknown_steps(c, &task.id);
    let verified: HashSet<_> = intent.verified_steps.iter().collect();
    if verified.iter().any(|step| !task.unknown_steps.iter().any(|unknown| &unknown.step == *step)) {
        return Err("待核实的步骤已变化，请重新打开任务确认".into());
    }
    task.unknown_steps.retain(|unknown| !verified.contains(&unknown.step));
    task.verified_steps = intent.verified_steps.clone();
    if !task.unknown_steps.is_empty() {
        task.evidence_status = "unknown_remote_state".into();
    }
    Ok(task)
}

pub(super) fn unknown_steps(c: &Conversation, task_id: &str) -> Vec<UnknownStep> {
    let mut steps = Vec::new();
    for message in &c.messages {
        let Some(task) = message.task.as_ref().filter(|task| task.id == task_id && !task.analysis_only) else {
            continue;
        };
        steps.extend(message.tools.iter().enumerate().filter_map(|(index, tool)| {
            (tool.status == "unknown" && !super::exec_access::verification_command(task.target_kind, &tool.arguments)).then(|| UnknownStep {
                step: StepRef { message_id: message.id.clone(), call_id: tool.call_id.clone() },
                run: task.run_number.max(1),
                index: index + 1,
                command: tool.arguments.get("command").or_else(|| tool.arguments.get("path")).and_then(serde_json::Value::as_str).unwrap_or(&tool.name).chars().take(160).collect(),
            })
        }));
    }
    let verified: HashSet<_> = c.messages.iter().filter_map(|message| message.task.as_ref())
        .filter(|task| task.id == task_id && !task.analysis_only)
        .flat_map(|task| &task.verified_steps)
        .collect();
    steps.retain(|unknown| !verified.contains(&unknown.step));
    steps
}

pub(super) fn has_unverified_change(c: &Conversation, kind: TargetKind, identity: &str, current: &str) -> bool {
    let matching = c.messages.iter().filter_map(|message| {
        let task = message.task.as_ref()?;
        (!task.analysis_only && task.id != current && task.target_kind == kind
            && task.target_identity.as_deref() == Some(identity)).then_some((message, task))
    });
    let verified: HashSet<_> = matching.clone().flat_map(|(_, task)| &task.verified_steps).collect();
    matching.into_iter().any(|(message, task)| message.tools.iter().any(|tool| {
        tool.status == "unknown" && !super::exec_access::verification_command(task.target_kind, &tool.arguments)
            && !verified.contains(&StepRef { message_id: message.id.clone(), call_id: tool.call_id.clone() })
    }))
}

pub(super) fn refresh_unknown_steps(c: &mut Conversation, task_id: &str) {
    let steps = unknown_steps(c, task_id);
    if let Some(task) = c.messages.iter_mut().rev().filter_map(|message| message.task.as_mut())
        .find(|task| task.id == task_id && !task.analysis_only)
    {
        if !steps.is_empty() {
            task.evidence_status = "unknown_remote_state".into();
        }
        task.unknown_steps = steps;
    }
}

pub(super) fn hydrate_unknown_steps(c: &mut Conversation) {
    let ids: HashSet<_> = c.messages.iter().filter_map(|message| {
        let task = message.task.as_ref()?;
        (!task.analysis_only && message.tools.iter().any(|tool| tool.status == "unknown")).then(|| task.id.clone())
    }).collect();
    for id in ids {
        refresh_unknown_steps(c, &id);
    }
}

pub(super) fn analysis_task(
    c: &Conversation,
    request: &SendRequest,
) -> Result<Option<Task>, String> {
    let Some(parent_id) = &request.analyze_message_id else {
        return Ok(None);
    };
    if request.diagnosis_target.is_some()
        || request.continue_diagnosis.is_some()
        || request.read_grant.is_some()
        || request.exec_grant.is_some()
    {
        return Err("继续分析不能同时启用执行工具".into());
    }
    let parent = c
        .messages
        .iter()
        .find(|message| message.id == *parent_id && message.role == "assistant")
        .ok_or("原诊断记录不存在")?;
    let old = parent.task.as_ref().ok_or("原回答不是诊断任务")?;
    let mut task = Task::new(
        &request.request_id,
        &old.goal,
        old.target.clone(),
        old.target_kind,
    );
    task.analysis_only = true;
    task.terminal_task = old.terminal_task;
    task.directory = old.directory.clone();
    task.skill = old.skill.clone();
    task.parent_message_id = Some(parent_id.clone());
    task.model_limit = 1;
    task.tool_limit = 0;
    task.command_limit = 0;
    task.evidence_status = old.evidence_status.clone();
    Ok(Some(task))
}

pub(super) fn analysis_context(c: &Conversation, task: &Task, limit: usize) -> String {
    let parent = c.messages.iter()
        .find(|message| Some(&message.id) == task.parent_message_id.as_ref())
        .and_then(|message| message.task.as_ref());
    let evidence = match parent {
        Some(parent) if !parent.analysis_only => evidence_preview(c, &parent.id, limit),
        Some(parent) => {
            let mut bytes = 0;
            let mut rows = Vec::new();
            for line in parent.context_snapshot.as_deref().unwrap_or_default().lines().rev() {
                if bytes + line.len() + 1 > limit {
                    break;
                }
                bytes += line.len() + 1;
                rows.push(line);
            }
            rows.reverse();
            rows.join("\n")
        }
        None => String::new(),
    };
    let target: String = task.target.label.chars().take(128).collect();
    let goal: String = task.goal.chars().take(256).collect();
    format!("原目标：{target}。原问题：{goal}。只根据下列有界证据分析。\n{evidence}")
}

pub(super) fn continuation_context(c: &Conversation, task: &Task, limit: usize) -> String {
    let kind = if task.terminal_task { "终端" } else { "诊断" };
    let verification = if !task.unknown_steps.is_empty() {
        "存在未核实的变更。只能提出受限只读检查，不能尝试其他变更。"
    } else if !task.verified_steps.is_empty() {
        "用户已确认核实此前结果未知的变更。结合用户的新说明和检查结果决定下一步。"
    } else {
        ""
    };
    let target: String = task.target.label.chars().take(128).collect();
    let goal: String = task.goal.chars().take(256).collect();
    format!("正在继续同一个{kind}任务。原目标：{target}。原问题：{goal}。{verification} 以下是历史数据，优先检查未确认的事项，引用实际步骤。\n{}",
        evidence_preview(c, &task.id, limit))
}

fn task_steps<'a>(
    c: &'a Conversation,
    task_id: &'a str,
) -> impl DoubleEndedIterator<Item = (usize, usize, &'a ToolRecord)> {
    c.messages
        .iter()
        .filter_map(move |message| {
            let task = message.task.as_ref()?;
            (task.id == task_id && !task.analysis_only)
                .then_some((task.run_number.max(1), &message.tools))
        })
        .flat_map(|(run, tools)| {
            tools
                .iter()
                .enumerate()
                .map(move |(step, tool)| (run, step + 1, tool))
        })
}

pub(super) fn evidence_preview(c: &Conversation, task_id: &str, limit: usize) -> String {
    let mut previews = Vec::new();
    let mut bytes = 0;
    let total = task_steps(c, task_id).count();
    for (run, step, tool) in task_steps(c, task_id).rev() {
        let clip = |text: &str| text.chars().take(512).collect::<String>();
        let execution = tool.execution.as_ref().map(|execution| {
            serde_json::json!({
                "outcome": execution.outcome,
                "exitCode": execution.exit_code,
                "signal": execution.signal,
                "endReason": execution.end_reason,
                "outputComplete": execution.output_complete,
                "stdoutPreview": clip(&execution.stdout),
                "stderrPreview": clip(&execution.stderr),
                "previewTruncated": execution.truncated
                    || execution.stdout.chars().nth(512).is_some()
                    || execution.stderr.chars().nth(512).is_some()
            })
        });
        let command = tool
            .arguments
            .get("command")
            .and_then(|value| value.as_str())
            .map(clip);
        let preview = serde_json::json!({
            "run": run,
            "step": step,
            "callId": tool.call_id,
            "tool": tool.name,
            "scope": clip(&tool.root),
            "status": tool.status,
            "observedAt": tool.started_at,
            "commandPreview": command,
            "execution": execution,
            "notice": tool.result.as_deref().map(clip)
        })
        .to_string();
        if bytes + preview.len() + 1 > limit || previews.len() >= 16 {
            break;
        }
        bytes += preview.len() + 1;
        previews.push(preview);
    }
    previews.reverse();
    format!(
        "历史共 {total} 条工具记录，以下为最近 {} 条的有界预览。\n{}",
        previews.len(),
        previews.join("\n")
    )
}

pub(super) async fn send(
    state: AiState,
    app: tauri::AppHandle,
    mut request: SendRequest,
    channel: tauri::ipc::Channel<AiEvent>,
) -> Result<Conversation, String> {
    let automatic = request.terminal_target.take();
    if automatic.is_some() && (request.diagnosis_target.is_some() || request.continue_diagnosis.is_some()
        || request.analyze_message_id.is_some() || request.deployment_target.is_some()
        || request.regenerate_id.is_some() || request.read_grant.is_some() || request.exec_grant.is_some()) {
        return Err("当前终端不能与其他任务目标混用".into());
    }
    if let Some(intent) = request.continue_diagnosis.clone() {
        if request.diagnosis_target.is_some()
            || request.regenerate_id.is_some()
            || request.analyze_message_id.is_some()
            || request.read_grant.is_some()
            || request.exec_grant.is_some()
        {
            return Err("继续检查不能混用新诊断、重新生成或其他工具授权".into());
        }
        let conversation_id = request.conversation_id.clone();
        request.diagnosis_target = Some(
            state
                .blocking(move |state| {
                    state.with_db(|db| {
                        let conversation = db.get(&conversation_id)?;
                        let (_, task) = continuation_source(&conversation, &intent)?;
                        Ok(RequestTarget {
                            id: intent
                                .rebind_session_id
                                .unwrap_or_else(|| task.target.id.clone()),
                            label: task.target.label.clone(),
                            kind: task.target_kind,
                            terminal_task: task.terminal_task,
                        })
                    })
                })
                .await?,
        );
    }
    if let Some(target) = automatic.as_ref().or(request.diagnosis_target.as_ref()) {
        if request.analyze_message_id.is_some()
            || request.read_grant.is_some()
            || request.exec_grant.is_some()
            || request.regenerate_id.is_some()
        {
            return Err("诊断模式不能混用手动工具或旧任务分析".into());
        }
        if target.id.is_empty()
            || target.id.len() > 256
            || target.label.len() > 256
            || target.label.chars().any(char::is_control)
        {
            return Err("诊断目标无效".into());
        }
        let prepared = super::exec_access::prepare_target(
            &state,
            app.clone(),
            target.id.clone(),
            request.model_id.clone(),
            target.kind,
            request.cmd_directory.take(),
        )
        .await;
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) if automatic.is_none() || request.skill.is_some() => return Err(error),
            Err(_) => return super::runtime::send(state, request, channel).await,
        };
        if let Some(mut target) = automatic {
            target.terminal_task = true;
            request.terminal_target = Some(target.clone());
            request.diagnosis_target = Some(target);
        }
        let token = prepared.token;
        request.exec_grant = Some(token.clone());
        let result = super::runtime::send_with_app(state.clone(), request, channel, Some(app)).await;
        if let Ok(mut grants) = state.exec_grants.lock() {
            grants.remove(&token);
        }
        result
    } else {
        super::runtime::send(state, request, channel).await
    }
}
