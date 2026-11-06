use super::{
    compatibility,
    types::{ModelConfig, Protocol, Service},
};
use rig::catalog::ModelSpec;
use rig::providers::registry::ProviderId;

pub const MARGIN: u64 = 1024;

// Resolve once for save, probe, chat and compaction. User values are never rewritten.
pub fn spec(service: &Service, model: &ModelConfig) -> ModelSpec {
    let mut spec =
        compatibility::model_spec(compatibility::adapter(service, model), &model.model_id)
            .cloned()
            .unwrap_or_else(|| {
                ModelSpec::new(
                    ProviderId::catalog(if service.protocol == Protocol::AnthropicMessages {
                        "anthropic"
                    } else {
                        "openai"
                    })
                    .expect("registered protocol provider"),
                    &model.model_id,
                )
            });
    spec.context_window = model
        .context_window
        .or(model.reported_context_window)
        .map(|v| v as u32)
        .or(spec.context_window);
    spec.max_input_tokens = model
        .reported_max_input_tokens
        .map(|v| v as u32)
        .or(spec.max_input_tokens);
    spec.max_output_tokens = model
        .max_output_limit
        .map(|v| v as u32)
        .or(spec.max_output_tokens);
    spec
}
fn minimum(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}
pub fn input_budget(service: &Service, model: &ModelConfig, output: u64) -> Result<usize, String> {
    let spec = spec(service, model);
    if let Some(limit) = spec.max_output_tokens {
        if output > u64::from(limit) {
            return Err(format!(
                "模型 {} 的最大输出超过已知上限 {limit} Token，请调整",
                model.name
            ));
        }
    }
    let window = spec
        .context_window
        .map(|w| u64::from(w).saturating_sub(output + MARGIN));
    let input = spec
        .max_input_tokens
        .map(|w| u64::from(w).saturating_sub(MARGIN));
    let budget = minimum(window, input).unwrap_or(16_384);
    if budget < 128 {
        return Err(format!(
            "模型 {} 的上下文窗口无法容纳输出预算及安全余量，请调整最大输出或窗口",
            model.name
        ));
    }
    Ok(budget as usize)
}
