<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { aiInvoke, useAiStore } from '@/stores/ai';
import { createService, createModel, serviceTemplates, adapterOptions, effectiveAdapter, capability, addDiscoveredModel, normalizeServiceDraft } from '@/utils/ai';
import { ChevronLeft, ChevronRight, Plus, RefreshCw, Star, Trash2, HelpCircle, Pencil } from '@lucide/vue';
import Button from '@/components/ui/button/Button.vue';
import Checkbox from '@/components/ui/checkbox/Checkbox.vue';
import Input from '@/components/ui/input/Input.vue';
import SettingsSelect from './SettingsSelect.vue';
import IconButton from '@/components/common/IconButton.vue';
import TooltipHint from '@/components/ui/tooltip/TooltipHint.vue';
import { confirm } from '@/composables/useConfirm';
import { toast } from '@/composables/useToast';

const store = useAiStore();
const pane = ref(null);
let disposed = false;
async function focusField(selector, origin = document.activeElement, reveal = false) {
  await nextTick();
  const active = document.activeElement;
  if (disposed || !pane.value?.getClientRects().length) return;
  if (active !== origin && active !== document.body) return;
  const target = typeof selector === 'function' ? selector() : pane.value.querySelector(selector);
  if (reveal) target?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  target?.focus({ preventScroll: true });
}

const form = ref(null);
const apiKey = ref('');
const expandedModel = ref('');
const busy = ref('');
const templateIndex = ref(serviceTemplates.length - 1);
const discovered = ref([]);
const discoveredQuery = ref('');
const dirty = ref(false);
const pendingDefault = ref(null);
const defaultModel = computed(() => {
  const id = pendingDefault.value || store.settings.defaultModelId;
  return form.value && !form.value.models.some(m => m.id === id && m.enabled) ? null : id;
});
const filteredModels = computed(() => discovered.value.filter(m => `${m.id} ${m.name || ''}`.toLowerCase().includes(discoveredQuery.value.toLowerCase())).slice(0, 100));
const profiles = computed(() => adapterOptions.filter(o => !o.value.startsWith('legacy_') && (form.value?.protocol === 'anthropic_messages' ? o.value === 'anthropic' || o.value === 'custom' : o.value !== 'anthropic')));
const modelProfiles = model => [{ value: null, label: '使用服务配置' }, ...profiles.value, ...adapterOptions.filter(o => o.value.startsWith('legacy_') && o.value === effectiveAdapter(form.value, model))];
const templateOptions = serviceTemplates.map((t,i) => ({value:i,label:t.name}));
const modeOptions = [{value:'default',label:'跟随服务'},{value:'enabled',label:'开启'},{value:'disabled',label:'关闭'}];
const capabilityOptions = [{value:'unknown',label:'自动'},{value:'supported',label:'支持'},{value:'unsupported',label:'不支持'}];
const catalog = ref({});
let catalogTimer; let catalogRevision = 0;
watch(() => form.value ? JSON.stringify(form.value.models.map(m => ({id:m.id,modelId:m.modelId,adapter:effectiveAdapter(form.value,m)}))) : '', value => {
  clearTimeout(catalogTimer); const revision = ++catalogRevision; catalog.value = {};
  if (!value) return;
  catalogTimer = setTimeout(async () => {
    try { const result = await aiInvoke('ai_model_catalog', {models:JSON.parse(value)}); if (revision === catalogRevision) catalog.value = result; } catch { /* Unavailable metadata remains unknown. */ }
  }, 250);
});
onBeforeUnmount(() => { disposed = true; clearTimeout(catalogTimer); catalogRevision++; });
const clone = value => JSON.parse(JSON.stringify(value));
const markDirty = () => { dirty.value = true; };
const profileFor = model => adapterOptions.find(o => o.value === effectiveAdapter(form.value, model));
const effortsFor = model => catalog.value[model.id]?.efforts || [];
const effortOptions = model => [{value:null,label:'默认'}, ...effortsFor(model).map(value => ({value,label:value}))];
const advancedEffortOptions = model => [{value:null,label:'默认'}, ...(profileFor(model)?.effort || []).map(value => ({value,label:value}))];
const budgetFor = model => catalog.value[model.id]?.budgetMin || (!catalog.value[model.id] && profileFor(model)?.budget ? 1024 : null);
const modesFor = model => modeOptions.map(option => ({ ...option, disabled: option.value !== 'default'
  && (capability(model, 'thinking', catalog.value[model.id]?.capabilities).state === 'unsupported'
    || (option.value === 'disabled' && catalog.value[model.id]?.canDisable === false)) }));
const keyHint = computed(() => form.value?.hasKey ? '已配置；留空保留原 Key，输入新值可替换' : '无认证服务可留空');

function edit(service) {
  form.value = clone(service);
  form.value.adapter ||= form.value.protocol === 'anthropic_messages' ? 'anthropic' : 'generic';
  form.value.models = form.value.models.map(m => ({ ...createModel(m.modelId), ...m,
    capabilities: { text: 'unknown', image: 'unknown', thinking: 'unknown', ...m.capabilities },
    reportedCapabilities: { text: 'unknown', image: 'unknown', thinking: 'unknown', ...m.reportedCapabilities },
    thinkingOnParams: m.thinkingOnParams ? JSON.stringify(m.thinkingOnParams, null, 2) : '',
    thinkingOffParams: m.thinkingOffParams ? JSON.stringify(m.thinkingOffParams, null, 2) : '',
  }));
  apiKey.value = ''; discovered.value = []; discoveredQuery.value = ''; expandedModel.value = '';
  dirty.value = false; pendingDefault.value = null;
}
async function discard() {
  if (!dirty.value) return true;
  try { await confirm({ title: '放弃未保存的服务配置？', content: '当前服务未保存的修改将丢弃。' }); return true; } catch { return false; }
}
async function editService(service) {
  const origin = document.activeElement;
  edit(service); await focusField('#ai-service-name', origin, true);
}
async function addService() {
  const origin = document.activeElement;
  if (!await discard()) return;
  edit(createService(serviceTemplates[templateIndex.value])); markDirty();
  await focusField('#ai-service-name', origin, true);
}
async function run(kind, action) {
  if (busy.value) return;
  const origin = document.activeElement;
  busy.value = kind;
  try { return await action(); } catch (e) { toast.error(String(e)); }
  finally {
    busy.value = '';
    if (origin?.isConnected && pane.value?.contains(origin)) {
      await focusField(() => origin.disabled ? pane.value.querySelector('#ai-service-name') : origin, origin);
    }
  }
}
async function save() {
  if (!form.value || !dirty.value) return;
  return run('save', async () => {
    const settings = await aiInvoke('ai_save_service', { service: normalizeServiceDraft(form.value), apiKey: apiKey.value || null, defaultModelId: pendingDefault.value });
    store.settings = settings;
    if (!store.selectedModel) store.selectedModelId = settings.defaultModelId;
    const saved = settings.services.find(s => s.id === form.value.id);
    if (saved) { const expanded = expandedModel.value; edit(saved); expandedModel.value = expanded; }
    toast.success('服务已保存');
  });
}
async function probe(kind, modelId = null) {
  await run(kind, async () => {
    // Validate only the fields relevant to this operation, including the current
    // unsaved model. An unfinished second model must not block listing/testing.
    const draft = clone(form.value);
    draft.models = kind === 'model' ? draft.models.filter(m => m.id === modelId) : [];
    const response = await aiInvoke('ai_probe', { draft: normalizeServiceDraft(draft), apiKey: apiKey.value || null, kind, modelId });
    if (kind === 'models') {
      discovered.value = response.result.models;
      toast.success(`已获取 ${discovered.value.length} 个模型`);
    } else {
      toast.success(`调用成功，${(response.elapsedMs / 1000).toFixed(2)} 秒；${response.result.reasoningChars ? `收到 ${response.result.reasoningChars} 字符思考内容` : '未收到可展示的思考内容'}`);
    }
  });
}
async function addModel(entry = null) {
  const origin = document.activeElement;
  if (entry && form.value.models.some(m => m.modelId === entry.id)) { toast.info('该模型已在目录中'); return; }
  const model = entry ? addDiscoveredModel(entry) : createModel();
  form.value.models.push(model); markDirty();
  if (!entry) {
    expandedModel.value = model.id;
    await focusField(() => [...pane.value.querySelectorAll('.model-card')].find(card => card.dataset.modelId === model.id)?.querySelector('input'), origin, true);
  }
}
async function remove(service) {
  try { await confirm({ title: '删除模型服务？', content: '将删除服务配置及系统凭据，已有聊天历史保留。' }); } catch { return; }
  await run('delete', async () => { store.settings = await aiInvoke('ai_delete_service', { id: service.id }); toast.success('服务已删除'); });
}
function setDefault(model) {
  if (!model.enabled) return;
  pendingDefault.value = model.id; markDirty();
}
async function removeModel(index) {
  const origin = document.activeElement;
  if (pendingDefault.value === form.value.models[index].id) pendingDefault.value = null;
  form.value.models.splice(index, 1); markDirty();
  const next = form.value.models[Math.min(index, form.value.models.length - 1)];
  await focusField(() => next
    ? [...pane.value.querySelectorAll('.model-card')].find(card => card.dataset.modelId === next.id)?.querySelector('input')
    : pane.value.querySelector('[aria-label="添加模型"]'), origin);
}
function setEnabled(model, enabled) {
  if (!enabled && pendingDefault.value === model.id) pendingDefault.value = null;
  markDirty();
}
function clearModelMetadata(model) {
  model.reportedCapabilities = { text: 'unknown', image: 'unknown', thinking: 'unknown' };
  model.maxOutputLimit = null;
  model.contextWindow = null;
  model.reportedContextWindow = null;
  model.reportedMaxInputTokens = null;
  markDirty();
}
async function back() {
  const origin = document.activeElement, id = form.value?.id;
  if (!await discard()) return;
  form.value = null; apiKey.value = ''; dirty.value = false; pendingDefault.value = null;
  await focusField(() => [...pane.value.querySelectorAll('.service-row')].find(row => row.dataset.serviceId === id)?.querySelector('button') || pane.value.querySelector('.add-service button'), origin);
}
function clearReportedMetadata() {
  form.value.models.forEach(m => {
    m.reportedCapabilities = { text: 'unknown', image: 'unknown', thinking: 'unknown' };
    m.reportedContextWindow = null; m.reportedMaxInputTokens = null; m.maxOutputLimit = null;
  });
}
function changeProtocol() {
  clearReportedMetadata();
  form.value.adapter = form.value.protocol === 'anthropic_messages' ? 'anthropic' : 'generic';
  form.value.models.forEach(m => { m.adapterOverride = null; m.thinkingFormat = ''; }); markDirty();
}
function changeModelAdapter(model, value) { model.reportedCapabilities = { text: 'unknown', image: 'unknown', thinking: 'unknown' }; model.reportedContextWindow = null; model.reportedMaxInputTokens = null; model.maxOutputLimit = null; model.adapterOverride = value || null; model.thinkingFormat = ''; model.reasoningEffort = null; markDirty(); }
function changeServiceAdapter() { clearReportedMetadata(); form.value.models.forEach(m => { if (!m.adapterOverride) { m.thinkingFormat = ''; m.reasoningEffort = null; } }); markDirty(); }
function capabilityLabels(model) {
  return [['text','文本'],['image','图片'],['thinking','思考']].map(([key,label]) => ({ key, label, ...capability(model,key,catalog.value[model.id]?.capabilities) }));
}
onMounted(() => run('load', () => store.loadSettings()));
defineExpose({ save, isDirty: () => dirty.value, isBusy: () => !!busy.value });
</script>

<template>
  <section ref="pane" class="ai-settings settings-content" :aria-busy="!!busy">
    <div class="pane-heading">
      <strong>模型服务</strong><Button v-if="form" size="sm" variant="ghost" :disabled="!!busy" @click="back"><ChevronLeft :size="14" />返回</Button>
    </div>
    <div v-if="!form" class="service-list" :inert="busy || undefined">
      <div v-for="service in store.settings.services" :key="service.id" :data-service-id="service.id" class="service-row"><strong>{{ service.name }}</strong><Button size="sm" variant="outline" @click="editService(service)"><Pencil :size="13" />编辑</Button><Button size="sm" variant="ghost" class="danger" @click="remove(service)">删除</Button></div>
      <div class="add-service"><SettingsSelect v-model="templateIndex" :options="templateOptions" aria-label="新服务模板" /><IconButton :icon="Plus" aria-label="添加模型提供商" class="outlined-icon" :action="addService" /></div>
    </div>
    <form v-else class="service-editor" :inert="busy || undefined" @input="markDirty" @change="markDirty" @submit.prevent="save" @keydown.enter="($event.target.tagName === 'INPUT') && $event.preventDefault()">
      <div class="settings-section">
        <div class="setting-row"><label for="ai-service-name" class="setting-label">服务名称</label><Input id="ai-service-name" v-model="form.name" size="sm" maxlength="100" /></div>
        <div class="setting-row"><span class="setting-label">API 协议</span><SettingsSelect v-model="form.protocol" :options="[{value:'openai_chat',label:'OpenAI Chat Completions'},{value:'anthropic_messages',label:'Anthropic Messages'}]" aria-label="API 协议" @update:model-value="changeProtocol" /></div>
        <div class="setting-row"><label for="ai-base-url" class="setting-label">Base URL</label><Input id="ai-base-url" v-model="form.baseUrl" @update:model-value="clearReportedMetadata" size="sm" spellcheck="false" placeholder="http://host:port/v1" /></div>
        <div class="setting-row"><div class="setting-label key-label"><label for="ai-api-key">API Key</label><TooltipHint :text="keyHint" side="right"><button type="button" class="key-help" aria-label="API Key 使用说明"><HelpCircle :size="13" /></button></TooltipHint></div><Input id="ai-api-key" v-model="apiKey" type="password" size="sm" autocomplete="new-password" spellcheck="false" maxlength="8192" /></div>
      </div>
      <details class="advanced-section"><summary><ChevronRight :size="13" />高级兼容设置</summary><div class="setting-row"><span class="setting-label">接口适配</span><SettingsSelect v-model="form.adapter" :options="profiles" aria-label="接口适配类型" @update:model-value="changeServiceAdapter" /></div></details>
      <div class="model-catalog">
        <div class="catalog-heading"><strong>模型目录</strong><div class="heading-actions"><IconButton :icon="RefreshCw" size="sm" aria-label="获取模型" class="outlined-icon" :action="() => probe('models')" /><IconButton :icon="Plus" size="sm" aria-label="添加模型" class="outlined-icon" :action="() => addModel()" /></div></div>
        <div v-if="discovered.length" class="discovered"><Input v-model="discoveredQuery" size="sm" @input.stop placeholder="搜索模型 ID 或名称" aria-label="搜索获取的模型" /><div class="model-results"><button v-for="m in filteredModels" :key="m.id" type="button" @click="addModel(m)"><span>{{ m.name || m.id }}</span><Plus :size="12" /></button></div></div>
        <div v-if="!form.models.length" class="empty-models">尚未添加模型</div>
        <div v-for="(model, i) in form.models" :key="model.id" :data-model-id="model.id" class="model-card">
          <div class="model-row"><Input v-model="model.modelId" size="sm" aria-label="模型 ID" @update:model-value="() => clearModelMetadata(model)" placeholder="模型 ID" maxlength="256" /><Input v-model="model.name" size="sm" aria-label="模型显示名称" placeholder="显示名称" maxlength="100" /><IconButton :icon="ChevronRight" size="sm" :aria-label="`配置模型 ${model.modelId || i + 1}`" :aria-expanded="expandedModel === model.id" :class="{ rotated: expandedModel === model.id }" :action="() => expandedModel = expandedModel === model.id ? '' : model.id" /><Checkbox v-model="model.enabled" aria-label="启用模型" @update:model-value="value => setEnabled(model, value)" /><IconButton :icon="Trash2" size="sm" tone="danger" aria-label="删除模型" :action="() => removeModel(i)" /></div>
          <div v-if="capabilityLabels(model).some(cap => cap.state === 'supported') || defaultModel === model.id" class="capabilities"><span v-for="cap in capabilityLabels(model).filter(cap => cap.state === 'supported')" :key="cap.key" class="capability">{{ cap.label }}</span><span v-if="defaultModel === model.id" class="default-badge"><Star :size="10" />默认</span></div>
          <div v-if="expandedModel === model.id" class="model-options">
            <div class="setting-row"><span class="setting-label">上下文窗口</span><Input v-model="model.contextWindow" size="sm" inputmode="numeric" :placeholder="model.reportedContextWindow ? `服务返回：${model.reportedContextWindow}` : catalog[model.id]?.contextWindow ? `内置目录：${catalog[model.id].contextWindow}` : '未知'" aria-label="上下文窗口" /></div>
            <div class="setting-row"><span class="setting-label">最大输出</span><Input v-model="model.maxOutputTokens" size="sm" inputmode="numeric" aria-label="最大输出 Token" /><TooltipHint v-if="model.maxOutputLimit || catalog[model.id]?.maxOutputTokens" :text="`${model.maxOutputLimit ? '服务上限' : '内置目录参考上限'}：${model.maxOutputLimit || catalog[model.id]?.maxOutputTokens} Token`"><HelpCircle :size="13" class="field-hint" /></TooltipHint></div>
            <p v-if="(model.maxOutputLimit || catalog[model.id]?.maxOutputTokens) && Number(model.maxOutputTokens) > (model.maxOutputLimit || catalog[model.id]?.maxOutputTokens)" class="danger">最大输出超过已知上限，请调整后保存。默认 256K 不代表服务支持该额度。</p>
            <div class="setting-row"><span class="setting-label">思考模式</span><SettingsSelect v-model="model.thinkingMode" :options="modesFor(model)" aria-label="思考模式" @update:model-value="markDirty" /></div>
            <div v-if="model.thinkingMode === 'enabled' && effortsFor(model).length" class="setting-row"><span class="setting-label">思考强度</span><SettingsSelect v-model="model.reasoningEffort" :options="effortOptions(model)" aria-label="思考强度" @update:model-value="markDirty" /></div>
            <div v-if="model.thinkingMode === 'enabled' && budgetFor(model)" class="setting-row"><span class="setting-label">思考预算</span><Input v-model="model.reasoningBudget" size="sm" inputmode="numeric" :placeholder="String(budgetFor(model))" aria-label="思考预算" /></div>
            <details class="advanced-section model-advanced"><summary><ChevronRight :size="13" />模型能力与兼容设置</summary>
              <div class="setting-row"><span class="setting-label">接口适配</span><SettingsSelect :model-value="model.adapterOverride || (form.protocol === 'openai_chat' && model.thinkingFormat ? effectiveAdapter(form, model) : null)" :options="modelProfiles(model)" aria-label="模型接口适配覆盖" @update:model-value="value => changeModelAdapter(model, value)" /></div>
              <div v-for="[key, label] in [['text','文本'],['image','图片'],['thinking','思考']]" :key="key" class="setting-row"><span class="setting-label">{{ label }}能力</span><SettingsSelect v-model="model.capabilities[key]" :options="capabilityOptions" :aria-label="`${label}能力`" @update:model-value="markDirty" /></div>
              <div v-if="model.thinkingMode === 'enabled' && !effortsFor(model).length && profileFor(model)?.effort?.length" class="setting-row"><span class="setting-label">思考强度</span><SettingsSelect v-model="model.reasoningEffort" :options="advancedEffortOptions(model)" aria-label="自定义模型思考强度" @update:model-value="markDirty" /></div>
              <template v-if="effectiveAdapter(form, model) === 'custom'"><label class="json-label">开启思考参数<textarea v-model="model.thinkingOnParams" rows="3" spellcheck="false" placeholder='{"thinking":{"type":"enabled"}}' /></label><label class="json-label">关闭思考参数<textarea v-model="model.thinkingOffParams" rows="3" spellcheck="false" placeholder='{"thinking":{"type":"disabled"}}' /></label></template>
            </details>
            <div class="model-actions"><Button type="button" size="sm" variant="outline" @click="probe('model', model.id)">实际调用测试</Button><Button type="button" size="sm" variant="outline" :disabled="!model.enabled || defaultModel === model.id" @click="setDefault(model)"><Star :size="12" />设为默认</Button></div>
          </div>
        </div>
      </div>
      <details class="advanced-section connection-options"><summary><ChevronRight :size="13" />连接选项</summary><div class="setting-row"><span class="setting-label">超时（秒）</span><Input v-model="form.timeoutSecs" size="sm" inputmode="numeric" aria-label="请求超时秒数" /></div><div class="setting-row"><span class="setting-label">代理</span><Input v-model="form.proxyUrl" size="sm" spellcheck="false" placeholder="可选，如 http://127.0.0.1:7890" aria-label="代理地址" /></div></details>
      <div class="editor-footer"><span v-if="busy" role="status" class="muted">{{ busy === 'save' ? '保存中…' : '请求中…' }}</span><Button type="submit" size="sm" :disabled="!dirty || !!busy">保存服务</Button></div>
    </form>
    <span v-if="busy && !form" role="status" class="muted">加载中…</span>
  </section>
</template>

<style scoped>
@import './settingsPaneShared.css';
.ai-settings{gap:12px;font-size:12px;color:var(--app-text)}.pane-heading,.catalog-heading,.heading-actions{display:flex;align-items:center;gap:8px}.pane-heading,.catalog-heading{justify-content:space-between;min-height:28px}.pane-heading strong,.catalog-heading strong{font-size:13px;font-weight:600}.service-list{display:flex;flex-direction:column;gap:10px}.service-row{display:flex;align-items:center;gap:8px;min-height:58px;border:1px solid var(--app-border-light);border-radius:12px;padding:12px}.service-row>strong{flex:1;min-width:0;font-size:13px;overflow-wrap:anywhere}.add-service{display:flex;align-items:center;gap:10px;padding:10px 12px;border:1px dashed var(--app-border-light);border-radius:10px}.add-service>:first-child{flex:1}.outlined-icon{border:1px solid var(--app-border-light);background:var(--app-input-bg);border-radius:6px}.service-editor{display:flex;flex-direction:column;gap:14px;min-width:0}.service-editor>.settings-section{gap:10px;margin:0}.setting-row{gap:10px;min-width:0}.setting-label{width:78px}.setting-row>:last-child:not(.field-hint){min-width:0}.key-label{display:flex;align-items:center;gap:3px;white-space:nowrap}.key-help{display:inline-flex;align-items:center;justify-content:center;width:18px;height:18px;flex-shrink:0;color:var(--app-text-muted);opacity:.7;border-radius:3px}.key-help:hover{opacity:1}.key-help:focus-visible{outline:none;box-shadow:var(--app-focus-shadow)}.advanced-section{border-top:1px solid var(--app-border-light);padding-top:6px}.advanced-section>summary{display:flex;align-items:center;gap:5px;list-style:none;cursor:pointer;color:var(--app-text-muted);font-size:11px;min-height:28px}.advanced-section>summary::-webkit-details-marker{display:none}.advanced-section[open]>summary>svg{transform:rotate(90deg)}.advanced-section>.setting-row{margin-top:8px}.model-catalog{display:flex;flex-direction:column;gap:9px}.catalog-heading{margin-bottom:1px}.empty-models{padding:18px 12px;border:1px dashed var(--app-border-light);border-radius:9px;color:var(--app-text-muted);text-align:center;font-size:11px}.model-card{border:1px solid var(--app-border-light);border-radius:10px;overflow:clip;background:var(--app-panel-bg,var(--app-bg))}.model-row{display:grid;grid-template-columns:minmax(0,1.15fr) minmax(0,1fr) 24px 28px 24px;gap:6px;align-items:center;padding:9px}.model-row:deep(input){background:var(--app-input-bg)}.model-row:deep([data-slot=checkbox]){justify-self:center}.rotated:deep(svg){transform:rotate(90deg)}.model-options{display:flex;flex-direction:column;gap:9px;border-top:1px solid var(--app-border-light);padding:12px}.model-options .setting-label{width:72px;font-size:11px}.model-actions{display:flex;gap:8px;flex-wrap:wrap;margin-top:3px}.model-advanced{margin-top:3px}.field-hint{flex:none;color:var(--app-text-muted)}.capabilities{display:flex;align-items:center;gap:5px;padding:0 10px 8px}.capability,.default-badge{display:inline-flex;align-items:center;gap:3px;font-size:10px;line-height:16px;padding:0 5px;border-radius:4px;background:var(--app-btn-hover);color:var(--app-text-muted)}.default-badge{color:var(--color-primary)}.discovered{padding:9px;border:1px solid var(--app-border-light);border-radius:9px}.model-results{display:flex;flex-direction:column;gap:2px;max-height:145px;overflow:auto;margin-top:7px}.model-results button{display:flex;align-items:center;justify-content:space-between;gap:10px;padding:5px 7px;border-radius:5px;text-align:left;color:var(--app-text-muted);font-size:11px}.model-results button span{overflow-wrap:anywhere;min-width:0}.model-results button:hover{background:var(--app-btn-hover);color:var(--app-text)}.json-label{display:flex;flex-direction:column;gap:5px;font-size:11px;color:var(--app-text-muted);margin-top:10px}textarea{width:100%;resize:vertical;border:1px solid var(--app-border-light);border-radius:7px;padding:7px 9px;background:var(--app-input-bg);color:var(--app-text);font-family:monospace;font-size:11px;outline:none}textarea:focus{border-color:var(--app-focus-border)}.editor-footer{display:flex;align-items:center;justify-content:flex-end;gap:10px;padding:12px 0 8px;border-top:1px solid var(--app-border-light)}.muted{color:var(--app-text-muted);font-size:11px}.danger{color:var(--color-destructive,#e16b6b)}
@media(max-width:640px){.model-row{grid-template-columns:minmax(0,1fr) minmax(0,1fr) 22px 26px 22px;gap:4px;padding:7px}.setting-label{width:68px}.model-options{padding:9px}}
</style>
