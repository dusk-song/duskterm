//! Model-generated deployment proposals. Preview data carries no tool permissions.
use super::{diagnosis::Target, types::SendRequest};
use serde::{Deserialize, Serialize};

pub(super) const PROMPT: &str = include_str!("prompts/deployment.md");
pub(super) const MAX_RESPONSE: usize = 32_768;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub target: Target,
    pub plan: Option<Plan>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Plan {
    pub summary: String,
    pub pending_inputs: Vec<String>,
    pub steps: Vec<Step>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Step {
    phase: Phase,
    title: String,
    detail: String,
    command: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Phase {
    Check,
    Change,
    Verify,
    Recovery,
}

pub(super) fn target(request: &SendRequest) -> Result<Option<&Target>, String> {
    let Some(target) = &request.deployment_target else {
        return Ok(None);
    };
    if request.read_grant.is_some()
        || request.exec_grant.is_some()
        || request.diagnosis_target.is_some()
        || request.continue_diagnosis.is_some()
        || request.cmd_directory.is_some()
        || request.analyze_message_id.is_some()
        || request.regenerate_id.is_some()
    {
        return Err("部署计划只能作为独立预览发起".into());
    }
    if !valid_text(&target.id, 256)
        || !valid_text(&target.label, 256)
        || target.id.chars().any(char::is_control)
        || target.label.chars().any(char::is_control)
    {
        return Err("部署计划的 SSH 目标无效".into());
    }
    Ok(Some(target))
}

fn valid_text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= limit
        && !value.chars().any(|c| {
            (c.is_control() && !matches!(c, '\n' | '\t' | '\r'))
                || ('\u{202a}'..='\u{202e}').contains(&c)
                || ('\u{2066}'..='\u{2069}').contains(&c)
        })
}

pub(super) fn parse(text: &str) -> Result<Plan, String> {
    let error = "模型未返回可审阅的部署计划，请补充条件后重新生成";
    if text.len() > MAX_RESPONSE || super::diagnosis::has_raw_tool_markup(text) {
        return Err(error.into());
    }
    let text = text.trim();
    let json = text
        .strip_prefix("```json\n")
        .or_else(|| text.strip_prefix("```json\r\n"))
        .and_then(|body| body.strip_suffix("```"))
        .unwrap_or(text);
    let plan: Plan = serde_json::from_str(json).map_err(|_| error)?;
    if !valid_text(&plan.summary, 2048)
        || plan.pending_inputs.len() > 8
        || plan
            .pending_inputs
            .iter()
            .any(|item| !valid_text(item, 512))
        || plan.steps.len() > 12
        || plan
            .steps
            .first()
            .is_none_or(|step| step.phase != Phase::Check)
        || !plan.steps.iter().any(|step| step.phase == Phase::Change)
        || !plan.steps.iter().any(|step| step.phase == Phase::Verify)
        || !plan.steps.iter().any(|step| step.phase == Phase::Recovery)
        || !plan
            .steps
            .windows(2)
            .all(|pair| pair[0].phase <= pair[1].phase)
        || plan.steps.iter().any(|step| {
            !valid_text(&step.title, 256)
                || !valid_text(&step.detail, 2048)
                || step
                    .command
                    .as_ref()
                    .is_some_and(|command| !valid_text(command, 4096))
        })
    {
        return Err(error.into());
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plan() -> serde_json::Value {
        json!({
            "summary": "部署待核实",
            "pendingInputs": ["确认目标系统"],
            "steps": [
                {"phase": "check", "title": "核实系统", "detail": "识别发行版"},
                {"phase": "change", "title": "安装软件", "detail": "核实系统后选择软件源"},
                {"phase": "verify", "title": "检查服务", "detail": "确认服务和监听端口"},
                {"phase": "recovery", "title": "恢复配置", "detail": "根据备份恢复"}
            ]
        })
    }

    #[test]
    fn deployment_plan_accepts_ordered_preview_with_missing_inputs() {
        let draft = parse(&format!("```json\n{}\n```", plan())).unwrap();
        assert_eq!(draft.steps.len(), 4);
        assert_eq!(draft.pending_inputs, ["确认目标系统"]);
        assert!(draft.steps[1].command.is_none());
    }

    #[test]
    fn deployment_plan_rejects_bad_order_and_claimed_execution() {
        let mut draft = plan();
        draft["steps"].as_array_mut().unwrap().swap(1, 2);
        assert!(parse(&draft.to_string()).is_err());
        let mut draft = plan();
        draft["steps"][1]["executed"] = json!(true);
        assert!(parse(&draft.to_string()).is_err());
        assert!(parse("<｜｜DSML｜｜ calls>").is_err());
    }
}
