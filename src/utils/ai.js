export const serviceTemplates = [
  { name: 'OpenAI', baseUrl: 'https://api.openai.com/v1', protocol: 'openai_chat', adapter: 'openai' },
  { name: 'Anthropic', baseUrl: 'https://api.anthropic.com', protocol: 'anthropic_messages', adapter: 'anthropic' },
  { name: 'DeepSeek', baseUrl: 'https://api.deepseek.com', protocol: 'openai_chat', adapter: 'deepseek' },
  { name: '通义千问（中国内地）', baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1', protocol: 'openai_chat', adapter: 'qwen' },
  { name: '智谱 GLM', baseUrl: 'https://open.bigmodel.cn/api/paas/v4', protocol: 'openai_chat', adapter: 'zai' },
  { name: 'Kimi（中国站）', baseUrl: 'https://api.moonshot.cn/v1', protocol: 'openai_chat', adapter: 'moonshot' },
  { name: 'SiliconFlow（中国站）', baseUrl: 'https://api.siliconflow.cn/v1', protocol: 'openai_chat', adapter: 'siliconflow' },
  { name: 'OpenRouter', baseUrl: 'https://openrouter.ai/api/v1', protocol: 'openai_chat', adapter: 'openrouter' },
  { name: '自定义', baseUrl: '', protocol: 'openai_chat', adapter: 'generic' },
];

export function createService(template = serviceTemplates[0]) {
  return { id: crypto.randomUUID(), ...template, hasKey: false,
    proxyUrl: null, timeoutSecs: 120, models: [] };
}

export function createModel(modelId = '') {
  return { id: crypto.randomUUID(), modelId, name: modelId, enabled: true,
    contextWindow: null, maxOutputTokens: 262144, thinkingMode: 'default', thinkingFormat: '',
    adapterOverride: null, reasoningEffort: null, reasoningBudget: null, thinkingOnParams: null, thinkingOffParams: null,
    capabilities: { text: 'unknown', image: 'unknown', thinking: 'unknown' },
    reportedCapabilities: { text: 'unknown', image: 'unknown', thinking: 'unknown' }, maxOutputLimit: null };
}

export function modelOptions(settings, query = '') {
  const needle = query.trim().toLowerCase();
  return settings.services.map(service => ({ ...service, models: service.models.filter(model => model.enabled
    && `${model.name} ${model.modelId} ${service.name}`.toLowerCase().includes(needle)) }))
    .filter(service => service.models.length);
}

export function formatTokens(value) { return value == null ? '—' : Number(value).toLocaleString(); }

// Both request identity and sequence matter: a late answer must not alter a newer run.
export function acceptsAiEvent(active, event) {
  return !!active && active.requestId === event.requestId
    && active.conversationId === event.conversationId && event.seq > active.seq;
}

export const adapterOptions = [
  { value: 'generic', label: '通用兼容接口', effort: [] },
  { value: 'openai', label: 'OpenAI', effort: ['minimal', 'low', 'medium', 'high', 'xhigh', 'max'] },
  { value: 'anthropic', label: 'Anthropic', budget: true },
  { value: 'deepseek', label: 'DeepSeek', effort: ['low', 'high', 'max'] },
  { value: 'qwen', label: '通义 / DashScope', effort: [] },
  { value: 'siliconflow', label: 'SiliconFlow', effort: [] },
  { value: 'vllm', label: 'vLLM / SGLang 聊天模板', effort: [] },
  { value: 'moonshot', label: 'Moonshot / Kimi', effort: ['low', 'high', 'max'] },
  { value: 'zai', label: '智谱 / Z.AI', effort: ['low', 'high', 'max'] },
  { value: 'openrouter', label: 'OpenRouter', effort: ['minimal', 'low', 'medium', 'high', 'xhigh', 'max'] },
  { value: 'custom', label: '自定义请求参数', effort: [] },
  { value: 'legacy_deepseek', label: '原配置：thinking 开关', effort: [] },
  { value: 'legacy_openai', label: '原配置：reasoning_effort', effort: ['low', 'medium', 'high'] },
  { value: 'legacy_qwen', label: '原配置：enable_thinking', effort: [] },
];

export function effectiveAdapter(service, model) {
  if (model.adapterOverride) return model.adapterOverride;
  if (service.protocol === 'anthropic_messages') return service.adapter === 'custom' ? 'custom' : 'anthropic';
  if (model.thinkingFormat) return `legacy_${model.thinkingFormat}`;
  return service.adapter || 'generic';
}

export function capability(model, key, preset = null) {
  const manual = model.capabilities?.[key] || 'unknown';
  if (manual !== 'unknown') return { state: manual, source: '手动设置' };
  const reported = model.reportedCapabilities?.[key] || 'unknown';
  if (reported !== 'unknown') return { state: reported, source: '服务返回' };
  if (preset?.[key] && preset[key] !== 'unknown') return { state: preset[key], source: '内置目录' };
  return { state: 'unknown', source: '未知' };
}

export function addDiscoveredModel(entry) {
  const model = createModel(entry.id);
  model.name = entry.name || entry.id;
  model.contextWindow = entry.contextWindow || null;
  model.maxOutputLimit = entry.maxOutputLimit || null;
  model.reportedCapabilities = { ...model.reportedCapabilities, ...entry.capabilities };
  return model;
}

export function normalizeServiceDraft(service) {
  const draft = JSON.parse(JSON.stringify(service));
  const integer = (value, label, optional = false) => {
    if (optional && (value === '' || value == null)) return null;
    if (!/^\d+$/.test(String(value)) || !Number.isSafeInteger(Number(value))) throw new Error(`${label}须为整数`);
    return Number(value);
  };
  draft.timeoutSecs = integer(draft.timeoutSecs, '请求超时');
  for (const model of draft.models) {
    model.contextWindow = integer(model.contextWindow, '上下文窗口', true);
    model.maxOutputTokens = integer(model.maxOutputTokens, '最大输出 Token');
    model.reasoningBudget = integer(model.reasoningBudget, '思考预算', true);
    for (const [field, label] of [['thinkingOnParams', '开启思考参数'], ['thinkingOffParams', '关闭思考参数']]) {
      if (typeof model[field] === 'string') {
        try { model[field] = model[field].trim() ? JSON.parse(model[field]) : null; }
        catch { throw new Error(`${label}不是有效 JSON`); }
      }
    }
  }
  return draft;
}
