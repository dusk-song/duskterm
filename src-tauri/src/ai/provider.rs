use super::{
    compatibility, config,
    types::{Capabilities, ListedModel, ModelConfig, Protocol, Service, Usage},
};
use rig::providers::{anthropic::AnthropicConfig, openai::OpenAIConfig};
use rig::{
    completion::{CompletionRequest, CompletionResponse},
    operation::Completion,
    DynModel,
};
use rig_reqwest::reqwest;
use rig_reqwest::ReqwestClient;
use std::time::Duration;
use zeroize::Zeroizing;

pub fn validate(service: &mut Service) -> Result<(), String> {
    if uuid::Uuid::parse_str(&service.id).is_err() {
        return Err("服务标识无效".into());
    }
    service.name = service.name.trim().to_string();
    if service.name.is_empty() || service.name.chars().count() > 100 {
        return Err("服务名称须为 1–100 个字符".into());
    }
    let mut url = url::Url::parse(service.base_url.trim()).map_err(|_| "Base URL 格式无效")?;
    if url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Base URL 必须包含主机，且不能包含账号、密码、查询参数或片段".into());
    }
    if !matches!(url.scheme(), "https" | "http") {
        return Err("地址必须使用 HTTP 或 HTTPS".into());
    }
    let path = url.path().trim_end_matches('/').to_string();
    if path.ends_with("/chat/completions")
        || path.ends_with("/messages")
        || path.ends_with("/responses")
    {
        return Err("请填写 API 根地址，不要包含 chat/completions、messages 或 responses".into());
    }
    url.set_path(&path);
    service.base_url = url.as_str().trim_end_matches('/').to_string();
    if !(15..=600).contains(&service.timeout_secs) {
        return Err("请求超时须为 15–600 秒".into());
    }
    if !compatibility::ADAPTERS.contains(&service.adapter.as_str()) {
        return Err("接口适配类型无效".into());
    }
    if let Some(proxy) = &service.proxy_url {
        if proxy.trim().is_empty() {
            service.proxy_url = None;
        } else {
            let p = url::Url::parse(proxy).map_err(|_| "代理地址格式无效")?;
            if !matches!(p.scheme(), "http" | "https" | "socks5" | "socks5h")
                || p.host_str().is_none()
                || !p.username().is_empty()
                || p.password().is_some()
            {
                return Err("代理须为 HTTP(S)/SOCKS5 地址，且不能含凭据".into());
            }
        }
    }
    let mut ids = std::collections::HashSet::new();
    let mut names = std::collections::HashSet::new();
    if service.models.len() > 2000 {
        return Err("单个服务最多配置 2000 个模型".into());
    }
    let snapshot = service.clone();
    for model in &mut service.models {
        model.model_id = model.model_id.trim().to_string();
        model.name = model.name.trim().to_string();
        if !["default", "enabled", "disabled"].contains(&model.thinking_mode.as_str())
            || !["", "deepseek", "openai", "qwen"].contains(&model.thinking_format.as_str())
        {
            return Err("思考模式配置无效".into());
        }
        let adapter = compatibility::adapter(&snapshot, model);
        if !compatibility::ADAPTERS.contains(&adapter) {
            return Err("模型接口适配类型无效".into());
        }
        if (service.protocol == Protocol::AnthropicMessages
            && !["anthropic", "custom"].contains(&adapter))
            || (service.protocol == Protocol::OpenaiChat && adapter == "anthropic")
        {
            return Err("接口适配类型与 API 协议不匹配".into());
        }
        if model
            .reasoning_effort
            .as_deref()
            .is_some_and(|v| !["minimal", "low", "medium", "high", "xhigh", "max"].contains(&v))
        {
            return Err("思考强度无效".into());
        }
        for caps in [&model.capabilities, &model.reported_capabilities] {
            if [&caps.text, &caps.image, &caps.thinking]
                .iter()
                .any(|v| !["unknown", "supported", "unsupported"].contains(&v.as_str()))
            {
                return Err("模型能力配置无效".into());
            }
        }
        if model
            .reasoning_budget
            .is_some_and(|v| !(1024..=2_000_000).contains(&v))
        {
            return Err("思考预算须为 1024–2000000 Token".into());
        }
        for value in [&model.thinking_on_params, &model.thinking_off_params]
            .into_iter()
            .flatten()
        {
            compatibility::validate_params(value)?;
        }
        let spec = compatibility::model_spec(adapter, &model.model_id);
        let budget_mode =
            adapter == "anthropic" && spec.is_none_or(|s| s.reasoning.budget().is_some());
        let budget_tokens = model.reasoning_budget.unwrap_or_else(|| {
            spec.and_then(|s| s.reasoning.budget())
                .map_or(1024, |b| *b.start())
        });
        if budget_mode
            && model.thinking_mode == "enabled"
            && model.max_output_tokens <= u64::from(budget_tokens)
        {
            return Err("思考预算须小于最大输出 Token".into());
        }

        if uuid::Uuid::parse_str(&model.id).is_err()
            || !ids.insert(model.id.clone())
            || !names.insert(model.model_id.clone())
        {
            return Err("模型标识无效或重复".into());
        }
        if model.model_id.is_empty()
            || model.model_id.len() > 256
            || model.name.is_empty()
            || model.name.len() > 400
        {
            return Err("请填写有效的模型 ID 与显示名称".into());
        }
        if !(128..=4_000_000).contains(&model.max_output_tokens) {
            return Err("输出预算须为 128–4000000 Token".into());
        }
        for (value, minimum, label) in [
            (model.context_window, 4096, "上下文窗口"),
            (model.reported_context_window, 4096, "服务上下文窗口"),
            (model.reported_max_input_tokens, 128, "服务输入上限"),
            (model.max_output_limit, 128, "服务输出上限"),
        ] {
            if value.is_some_and(|v| !(minimum..=4_000_000).contains(&v)) {
                return Err(format!("{label}超出有效范围"));
            }
        }
        config::input_budget(&snapshot, model, model.max_output_tokens)?;
        compatibility::configure(CompletionRequest::new("validate"), model, &snapshot)?;
    }
    Ok(())
}

pub fn entry(id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new("com.duskterm.ai", id).map_err(|_| "无法访问系统凭据库".to_string())
}

pub fn credential(service: &Service) -> Result<Zeroizing<String>, String> {
    if !service.has_key {
        return Ok(Zeroizing::new(String::new()));
    }
    stored_credential(&service.id)?
        .ok_or_else(|| "系统凭据库中找不到此服务的 API Key，请重新保存".into())
}

pub fn stored_credential(id: &str) -> Result<Option<Zeroizing<String>>, String> {
    match entry(id)?.get_password() {
        Ok(key) => Ok(Some(Zeroizing::new(key))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("无法读取系统凭据库，请解锁或启用系统密钥服务".into()),
    }
}

fn raw_http(service: &Service) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(service.timeout_secs));
    if let Some(proxy) = &service.proxy_url {
        builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|_| "代理配置无效")?);
    }
    builder
        .build()
        .map_err(|_| "无法初始化模型网络客户端".to_string())
}
fn http(service: &Service) -> Result<ReqwestClient, String> {
    raw_http(service).map(ReqwestClient::from)
}

pub fn model(
    service: &Service,
    model: &ModelConfig,
    key: &str,
) -> Result<DynModel<Completion>, String> {
    config::input_budget(service, model, model.max_output_tokens)?;
    let transport = http(service)?;
    Ok(match service.protocol {
        Protocol::OpenaiChat => OpenAIConfig::with_key(
            dialect(compatibility::adapter(service, model)),
            key.to_string(),
        )
        .with_base_url(&service.base_url)
        .connect(transport)
        .chat(&model.model_id)
        .with_idle_timeout(Some(Duration::from_secs(service.timeout_secs.min(120))))
        .erase(),
        Protocol::AnthropicMessages => AnthropicConfig::new(key.to_string())
            .with_base_url(&service.base_url)
            .connect(transport)
            .completion(&model.model_id)
            .with_idle_timeout(Some(Duration::from_secs(service.timeout_secs.min(120))))
            .erase(),
    })
}

fn dialect(adapter: &str) -> &'static rig::providers::openai::wire::Dialect {
    use rig::providers::openai::wire;
    match adapter {
        "deepseek" => &wire::DEEPSEEK,
        "moonshot" => &wire::MOONSHOT,
        "zai" => &wire::ZAI,
        "openrouter" => &wire::OPENROUTER,
        _ => &wire::OPENAI,
    }
}

pub async fn list(service: &Service, key: &str) -> Result<Vec<ListedModel>, String> {
    // Rig's ModelInfo intentionally omits modality metadata. Parse the listing
    // here once, preserving declared capabilities without inferring from IDs.
    let suffix =
        if service.protocol == Protocol::AnthropicMessages && !service.base_url.ends_with("/v1") {
            "/v1/models"
        } else {
            "/models"
        };
    let mut request = raw_http(service)?.get(format!("{}{suffix}", service.base_url));
    if !key.is_empty() {
        request = if service.protocol == Protocol::AnthropicMessages {
            request.header("x-api-key", key)
        } else {
            request.bearer_auth(key)
        };
    }
    if service.protocol == Protocol::AnthropicMessages {
        request = request.header("anthropic-version", "2023-06-01");
    }
    let response = request.send().await.map_err(|error| {
        if error.is_timeout() {
            "TIMEOUT: 获取模型列表超时"
        } else {
            "NETWORK: 获取模型列表失败，请检查网络、代理和证书"
        }
    })?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(match status {
            401 => "AUTH: API Key 无效或已过期",
            403 => "PERMISSION: 无权获取模型列表",
            404 => "NOT_FOUND: 模型列表接口不存在，可手动添加模型",
            429 => "RATE_LIMIT: 请求受限，请稍后重试",
            _ => "SERVICE: 模型列表请求失败",
        }
        .into());
    }
    if response
        .content_length()
        .is_some_and(|n| n > 8 * 1024 * 1024)
    {
        return Err("模型目录响应过大".into());
    }
    use futures::StreamExt;
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "INCOMPLETE: 模型目录响应中断")?;
        if body.len() + chunk.len() > 8 * 1024 * 1024 {
            return Err("模型目录响应过大".into());
        }
        body.extend_from_slice(&chunk);
    }
    let value: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|_| "INCOMPLETE: 模型列表不是有效 JSON，请检查 API 根地址")?;
    let rows = value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(|v| v.as_array())
        .ok_or("INCOMPLETE: 无法识别模型列表格式")?;
    Ok(rows.iter().take(5000).filter_map(listed_model).collect())
}

fn listed_model(row: &serde_json::Value) -> Option<ListedModel> {
    let id = row.get("id")?.as_str()?.to_string();
    let mut capabilities = Capabilities::default();
    if let Some(inputs) = row
        .pointer("/architecture/input_modalities")
        .or_else(|| row.get("input_modalities"))
        .and_then(|v| v.as_array())
    {
        capabilities.text = if inputs.iter().any(|v| v == "text") {
            "supported"
        } else {
            "unsupported"
        }
        .into();
        capabilities.image = if inputs.iter().any(|v| v == "image") {
            "supported"
        } else {
            "unsupported"
        }
        .into();
    }
    if let Some(flag) = row
        .pointer("/capabilities/reasoning")
        .and_then(|v| v.as_bool())
    {
        capabilities.thinking = if flag { "supported" } else { "unsupported" }.into();
    } else if row
        .get("supported_parameters")
        .and_then(|v| v.as_array())
        .is_some_and(|a| {
            a.iter()
                .any(|v| v == "reasoning" || v == "reasoning_effort" || v == "thinking")
        })
    {
        capabilities.thinking = "supported".into();
    }
    Some(ListedModel {
        id,
        name: row
            .get("name")
            .or_else(|| row.get("display_name"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        max_input_tokens: row
            .get("max_input_tokens")
            .and_then(|v| v.as_u64())
            .filter(|v| (128..=4_000_000).contains(v)),
        context_window: row
            .get("context_length")
            .or_else(|| row.get("context_window"))
            .and_then(|v| v.as_u64())
            .filter(|n| (4096..=4_000_000).contains(n)),
        max_output_limit: row
            .get("max_output_tokens")
            .or_else(|| row.pointer("/top_provider/max_completion_tokens"))
            .and_then(|v| v.as_u64())
            .filter(|n| (128..=4_000_000).contains(n)),
        capabilities,
    })
}

pub async fn verify(service: &Service, key: &str) -> Result<(), String> {
    let transport = http(service)?;
    match service.protocol {
        Protocol::OpenaiChat => {
            OpenAIConfig::new(key.to_string())
                .with_base_url(&service.base_url)
                .connect(transport)
                .verify()
                .await
        }
        Protocol::AnthropicMessages => {
            AnthropicConfig::new(key.to_string())
                .with_base_url(&service.base_url)
                .connect(transport)
                .verify()
                .await
        }
    }
    .map_err(error)
}

pub async fn test(
    service: &Service,
    config: &ModelConfig,
    key: &str,
) -> Result<(Usage, usize), String> {
    let response = model(service, config, key)?
        .call(configure_request(
            CompletionRequest::new("Reply with OK only.").max_tokens(config.max_output_tokens),
            config,
            service,
        )?)
        .await
        .map_err(error)?;
    if response.text().trim().is_empty() {
        return Err("模型未返回文本，请检查模型能力和输出预算".into());
    }
    Ok((usage(&response), reasoning_chars(&response)))
}

pub fn configure_request(
    request: CompletionRequest,
    config: &ModelConfig,
    service: &Service,
) -> Result<CompletionRequest, String> {
    compatibility::configure(request, config, service)
}

pub fn reasoning_chars(response: &CompletionResponse) -> usize {
    response
        .choice
        .iter()
        .map(|part| match part {
            rig::message::AssistantContent::Reasoning(reasoning) => reasoning.text.chars().count(),
            _ => 0,
        })
        .sum()
}
pub fn usage(response: &CompletionResponse) -> Usage {
    Usage {
        input_tokens: response.usage.input_tokens,
        output_tokens: response.usage.output_tokens,
        total_tokens: response.usage.total_tokens,
        cached_input_tokens: response.usage.cached_input_tokens,
        cache_creation_input_tokens: response.usage.cache_creation_input_tokens,
        reasoning_tokens: response.usage.reasoning_tokens,
    }
}

// Reports can contain headers, raw bodies and echoed user content: never serialize or log them.
pub fn error(error: rig::ProviderError) -> String {
    if matches!(error, rig::ProviderError::UnsupportedOption(_)) {
        return "UNSUPPORTED: 此适配器或模型不支持所选思考参数，请调整高级兼容设置".into();
    }
    if matches!(error, rig::ProviderError::Stalled(_)) {
        return "STALLED: 模型服务长时间未返回数据，请重试或调整连接超时".into();
    }
    let report = error.report();
    let code = report.code.as_deref().unwrap_or("").to_lowercase();
    let (tag, message) =
        if report.is_context_overflow() || code.contains("context") || code.contains("token_limit")
        {
            (
                "CONTEXT_LIMIT",
                "上下文超出模型限制，请调整窗口配置、缩短问题或新建对话",
            )
        } else if code.contains("quota") || code.contains("balance") || code.contains("credit") {
            ("QUOTA", "服务余额或额度不足，请检查服务账户")
        } else {
            match report.http_status {
                Some(401) => ("AUTH", "API Key 无效或已过期"),
                Some(403) => ("PERMISSION", "服务拒绝访问，请检查权限、地域和模型授权"),
                Some(402) => ("QUOTA", "余额或额度不足"),
                Some(404) => (
                    "NOT_FOUND",
                    "端点或模型不存在，请检查协议、Base URL 和模型 ID",
                ),
                Some(429) => ("RATE_LIMIT", "请求受限或额度不足，请稍后重试并检查账户额度"),
                Some(400 | 422) => (
                    "UNSUPPORTED",
                    "请求参数、上下文长度或模型能力不兼容，请检查配置",
                ),
                Some(500..=599) => ("SERVICE", "模型服务暂时不可用，请稍后重试"),
                _ => match report.kind {
                    rig::ErrorKind::Timeout => ("TIMEOUT", "模型请求超时，可保留内容后重试"),
                    rig::ErrorKind::Http => {
                        ("NETWORK", "网络或证书校验失败，请检查网络、代理和系统证书")
                    }
                    _ => ("INCOMPLETE", "服务响应中断或协议不兼容，请检查配置后重试"),
                },
            }
        };
    format!("{tag}: {message}")
}
