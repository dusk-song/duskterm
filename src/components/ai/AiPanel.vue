<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { Maximize2, Minimize2, Plus, History, X, ArrowUp, Square, ChevronDown, Check } from '@lucide/vue';
import { onClickOutside } from '@vueuse/core';
import IconButton from '@/components/common/IconButton.vue';
import { Button } from '@/components/ui/button';
import TooltipHint from '@/components/ui/tooltip/TooltipHint.vue';
import { conversationUsage } from '@/utils/aiUsage';
import { formatTokens, modelOptions } from '@/utils/ai';
import { useAiStore } from '@/stores/ai';
import { confirm } from '@/composables/useConfirm';
import AiMessage from './AiMessage.vue';

const props = defineProps({ maximized: Boolean, visible: { type: Boolean, default: true } });
defineEmits(['close', 'maximize']);
const store = useAiStore();
const composer = ref(null);
const creating = ref(false);
let navigation = 0;
let disposed = false;
async function focusComposer(origin = document.activeElement) {
  await nextTick();
  const active = document.activeElement;
  if (disposed || !props.visible || !composer.value?.getClientRects().length) return;
  if (active !== origin && active !== document.body && active !== composer.value) return;
  composer.value.focus({ preventScroll: true });
}

const historyOpen = ref(false);
const pickerOpen = ref(false);
const modelContainer = ref(null);
onClickOutside(modelContainer, () => { pickerOpen.value = false; });
async function toggleModelPicker() {
  pickerOpen.value = !pickerOpen.value;
  if (!pickerOpen.value) return;
  historyOpen.value = false;
  await nextTick();
  const container = modelContainer.value;
  (container?.querySelector('.model-list button.selected') || container?.querySelector('.model-list button'))?.focus({ preventScroll: true });
}
async function closeModelPicker() {
  pickerOpen.value = false;
  await nextTick();
  modelContainer.value?.querySelector('.model-toggle')?.focus({ preventScroll: true });
}
function handleModelEscape(event) {
  if (!pickerOpen.value) return;
  event.preventDefault(); event.stopPropagation();
  void closeModelPicker();
}
function moveModelFocus(event) {
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
  const buttons = [...modelContainer.value.querySelectorAll('.model-list button')];
  if (!buttons.length) return;
  event.preventDefault();
  const index = buttons.indexOf(document.activeElement);
  const next = event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1
    : (index + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length;
  buttons[next].focus();
}
function selectModel(id) {
  const origin = document.activeElement;
  store.selectedModelId = id; pickerOpen.value = false;
  void focusComposer(origin);
}
watch(() => props.visible, visible => {
  if (!visible) { pickerOpen.value = false; historyOpen.value = false; navigation++; }
});
const emptyDraft = ref('');
const loading = ref(true);
const scroller = ref(null);
const followBottom = ref(true);
const options = computed(() => modelOptions(store.settings));
const usage = computed(() => conversationUsage(store.current));
const draft = computed({ get: () => store.current?.draft ?? emptyDraft.value, set: text => {
  if (store.current) store.current.draft = text; else emptyDraft.value = text;
} });
const lastUserId = computed(() => store.current?.messages.filter(m => m.role === 'user').at(-1)?.id);
const visibleTurns = ref(30);
const turns = computed(() => {
  const all = store.current?.messages || [];
  const replies = new Map();
  for (const message of all) {
    if (!message.replyTo) continue;
    if (!replies.has(message.replyTo)) replies.set(message.replyTo, []);
    replies.get(message.replyTo).push(message);
  }
  return all.filter(m => m.role === 'user').map(user => [user, ...(replies.get(user.id) || [])]);
});
const messages = computed(() => turns.value.slice(-visibleTurns.value).flat());
async function showOlder() {
  const height = scroller.value?.scrollHeight || 0;
  followBottom.value = false;
  visibleTurns.value += 30;
  await nextTick();
  if (scroller.value) scroller.value.scrollTop += scroller.value.scrollHeight - height;
}
watch(() => store.current?.id, (id, previous) => {
  visibleTurns.value = 30; followBottom.value = true;
  if (!previous && id && store.active?.conversationId === id && emptyDraft.value) {
    store.current.draft = emptyDraft.value; emptyDraft.value = '';
  }
});
const generatingHere = computed(() => !!store.active && store.active.conversationId === store.current?.id);
const draftWrites = new Map();
const draftTimers = new Map();
const pendingDrafts = new Map();
const sending = ref(false);
async function writeDraft(id, text) {
  // Serialize writes per conversation so switching quickly cannot restore an old draft.
  const pending = (draftWrites.get(id) || Promise.resolve()).catch(() => {}).then(() => store.saveDraft(id, text));
  draftWrites.set(id, pending);
  try { await pending; } catch (e) { store.error = String(e); }
  finally { if (draftWrites.get(id) === pending) draftWrites.delete(id); }
}
function flushDrafts() {
  for (const [id, text] of pendingDrafts) { clearTimeout(draftTimers.get(id)); void writeDraft(id, text); }
  pendingDrafts.clear(); draftTimers.clear();
  return Promise.all([...draftWrites.values()].map(p => p.catch(() => {})));
}
watch(() => [store.current?.id, draft.value], ([id, text]) => {
  if (!id) return;
  clearTimeout(draftTimers.get(id)); pendingDrafts.set(id, text);
  draftTimers.set(id, setTimeout(() => { pendingDrafts.delete(id); draftTimers.delete(id); void writeDraft(id, text); }, 300));
});
onBeforeUnmount(() => { disposed = true; navigation++; void flushDrafts(); });
async function openConversation(id) {
  const revision = ++navigation, origin = document.activeElement;
  await flushDrafts();
  if (revision !== navigation) return;
  const opened = await store.open(id);
  if (opened && revision === navigation) { historyOpen.value = false; await focusComposer(origin); }
}
async function newConversation() {
  if (creating.value) return;
  const revision = ++navigation, origin = document.activeElement;
  creating.value = true;
  try {
    await flushDrafts();
    if (revision !== navigation) return;
    const conversation = await store.create();
    if (revision === navigation && store.current?.id === conversation.id) {
      historyOpen.value = false; pickerOpen.value = false;
      await focusComposer(origin);
    }
  } finally { creating.value = false; }
}
async function stopGeneration() {
  const origin = document.activeElement;
  await store.stop(); await focusComposer(origin);
}
async function safe(action) { try { await action(); } catch (e) { if (e) store.error = String(e); } }
async function send(regenerateId = null) {
  if (sending.value || store.active || !store.selectedModel || (!regenerateId && !draft.value.trim())) return;
  sending.value = true;
  const sourceId = store.current?.id;
  const origin = document.activeElement;
  try {
    const selectedId = store.selectedModelId;
    const service = store.selectedModel.service;
    const oldService = store.current?.lastServiceId;
    if (oldService && oldService !== service.id) {
      try { await confirm({ title: `发送到 ${service.name}？`, content: '本次问题、有效历史对话及摘要将发送到该服务。切换服务不会删除本地历史。', okText: '发送' }); } catch { return; }
    }
    const text = draft.value;
    await flushDrafts();
    if (store.current?.id !== sourceId || store.selectedModelId !== selectedId) return;
    if (!regenerateId) draft.value = '';
    followBottom.value = true;
    const run = store.send(text, regenerateId);
    await focusComposer(origin);
    const accepted = await run;
    // Validation failures before a persisted user message should retain the input.
    if (!regenerateId && !accepted && store.current?.id === sourceId) draft.value ||= text;
    if (!regenerateId && !accepted && !sourceId && !store.current) emptyDraft.value ||= text;
  } finally { sending.value = false; }
}
let scrollFrame = null;
function scheduleScroll() {
  if (!followBottom.value || scrollFrame !== null) return;
  scrollFrame = requestAnimationFrame(() => {
    scrollFrame = null;
    if (followBottom.value && scroller.value) scroller.value.scrollTop = scroller.value.scrollHeight;
  });
}
watch(() => [store.current?.id, store.current?.messages.length,
  store.current?.messages.at(-1)?.content, store.current?.messages.at(-1)?.reasoning], scheduleScroll, { flush: 'post' });
const messageContent = ref(null);
let resizeObserver;
onMounted(() => {
  resizeObserver = new ResizeObserver(scheduleScroll);
  if (messageContent.value) resizeObserver.observe(messageContent.value);
});
onBeforeUnmount(() => { resizeObserver?.disconnect(); if (scrollFrame !== null) cancelAnimationFrame(scrollFrame); });
onMounted(async () => { await safe(() => store.initialize()); loading.value = false; });
</script>

<template>
  <section class="ai-panel" aria-label="智能助手">
    <header>
      <strong>智能助手</strong>
      <div class="header-actions">
        <IconButton :icon="Plus" size="sm" :disabled="creating || loading" :aria-busy="creating" aria-label="新建对话" :action="() => safe(newConversation)" />
        <IconButton :icon="History" size="sm" aria-label="历史对话" :aria-expanded="historyOpen" :active="historyOpen" :action="() => { historyOpen = !historyOpen; pickerOpen = false; }" />
        <IconButton :icon="props.maximized ? Minimize2 : Maximize2" size="sm" :aria-label="maximized ? '恢复面板' : '工作区内最大化'" :action="() => $emit('maximize')" />
        <IconButton :icon="X" size="sm" aria-label="关闭面板" :action="() => $emit('close')" />
      </div>
    </header>
    <div v-if="historyOpen" class="history" @keydown.esc.stop.prevent="historyOpen = false; focusComposer()">
      <p v-if="!store.conversations.length" class="muted">暂无历史对话</p>
      <button v-for="c in store.conversations" :key="c.id" class="history-title" :class="{ selected: c.id === store.current?.id }" @click="safe(() => openConversation(c.id))">{{ c.title }}</button>
    </div>
    <div v-if="store.active && !generatingHere" class="background-status" role="status">另一个对话正在生成 <Button variant="ghost" size="sm" @click="safe(() => openConversation(store.active.conversationId))">查看</Button><Button variant="ghost" size="sm" @click="safe(stopGeneration)">停止</Button></div>
    <div ref="scroller" class="messages" @scroll="followBottom = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 80">
      <div ref="messageContent" class="message-content">
      <Button v-if="turns.length > visibleTurns" variant="ghost" size="sm" @click="safe(showOlder)">加载更早的消息</Button>
      <div v-if="loading" class="empty">正在读取本地对话…</div>
      <div v-else-if="!store.settings.services.length || !modelOptions(store.settings).length" class="empty"><strong>配置你的 AI 模型</strong><p>请进入「首选项 → 模型服务」，新增服务并启用模型。</p><p>AI 可独立使用，无需连接 SSH。</p></div>
      <div v-else-if="!messages.length" class="empty"><strong>有什么可以帮你？</strong><p>输入问题，或粘贴代码和报错。</p></div>
      <AiMessage v-for="message in messages" :key="message.id" :message="message" :busy="!!store.active" :can-regenerate="message.role === 'assistant' && message.replyTo === lastUserId && message.status !== 'streaming'" @regenerate="safe(() => send(message.id))" @select="safe(() => store.selectAnswer(message.id))" />
      <p v-if="generatingHere && ['context','started'].includes(store.active.stage)" class="muted" role="status">正在准备上下文，长会话可能需要调用模型生成摘要…</p>
      </div>
    </div>
    <p v-if="store.error" class="error" role="alert">{{ store.error }}</p>
    <div class="composer">
      <div class="input-shell">
        <textarea ref="composer" v-model="draft" aria-label="AI 消息" placeholder="输入问题，或粘贴代码和报错…" rows="3" maxlength="128000" @keydown.enter.exact="if (!$event.isComposing) { $event.preventDefault(); safe(() => send()); }" />
        <div class="input-actions">
          <div ref="modelContainer" class="model-container" @keydown.esc="handleModelEscape">
            <TooltipHint :text="store.selectedModel ? `${store.selectedModel.service.name}（${store.selectedModel.modelId}）` : '请选择模型'">
              <Button variant="ghost" size="sm" class="model-toggle" :aria-expanded="pickerOpen" aria-haspopup="dialog" :disabled="loading || !options.length" @click="toggleModelPicker"><span>{{ store.selectedModel?.name || '选择模型' }}</span><ChevronDown :size="12" /></Button>
            </TooltipHint>
            <div v-if="pickerOpen" class="model-picker" role="dialog" aria-label="选择模型">
              <div class="model-list" @keydown="moveModelFocus">
                <div v-for="service in options" :key="service.id">
                  <p>{{ service.name }}</p>
                  <button v-for="model in service.models" :key="model.id" type="button" :class="{ selected: store.selectedModelId === model.id }" :aria-pressed="store.selectedModelId === model.id" @click="selectModel(model.id)">
                    <span>{{ model.name }}<small v-if="model.name !== model.modelId">{{ model.modelId }}</small></span><Check v-if="store.selectedModelId === model.id" :size="14" />
                  </button>
                </div>
                <p v-if="!options.length" class="muted">暂无已启用模型</p>
              </div>
            </div>
          </div>
          <IconButton v-if="generatingHere" :icon="Square" size="28px" class="send stop" aria-label="停止生成" tooltip-side="top" :action="() => safe(stopGeneration)" />
          <IconButton v-else :icon="ArrowUp" size="28px" class="send" aria-label="发送消息" tooltip-side="top" :disabled="sending || !!store.active || !store.selectedModel || !draft.trim()" :action="() => safe(() => send())" />
        </div>
      </div>
      <div class="conversation-usage" aria-label="当前对话用量" aria-live="polite">
        <TooltipHint :text="usage.partial ? '当前对话已知用量，包含重新生成和摘要调用；部分调用或旧历史未报告完整用量。' : '当前对话累计用量，包含重新生成和摘要调用。'">
          <span>{{ usage.partial ? '已知' : '累计' }} Token {{ formatTokens(usage.totalTokens) }}</span>
        </TooltipHint>
        <TooltipHint v-if="usage.cacheRate !== null" :text="usage.cachePartial ? '仅统计已报告缓存数据的调用，以命中输入 Token / 输入 Token 计算。' : '缓存命中输入 Token / 输入 Token；不将缓存 Token 重复计入总用量。'">
          <span class="cache-rate">缓存命中率 {{ usage.cacheRate.toFixed(1) }}%{{ usage.cachePartial ? '（已报告）' : '' }}</span>
        </TooltipHint>
      </div>
    </div>
  </section>
</template>

<style scoped>
.ai-panel{height:100%;min-height:0;display:flex;flex-direction:column;color:var(--app-text);font-size:12px;background:var(--app-panel-bg)}
header{display:flex;align-items:center;justify-content:space-between;padding:10px 12px;border-bottom:1px solid var(--app-border-light)}.header-actions{display:flex;gap:4px}
.messages{flex:1;min-height:0;overflow:auto;padding:0 14px;scrollbar-gutter:stable}
.message-content{min-height:100%}
.empty{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:10px;min-height:100%;text-align:center;color:var(--app-text-muted);line-height:1.6;padding:24px 8px}.empty strong{color:var(--app-text);font-size:15px}
.composer{padding:10px 12px 8px}.input-shell{border:1px solid var(--app-input-border);border-radius:10px;background:var(--app-input-bg)}.input-shell:focus-within{border-color:var(--app-focus-border)}
textarea{width:100%;resize:vertical;max-height:200px;min-height:65px;background:transparent;border:0;padding:10px;color:var(--app-text);font-size:12px;outline:none}textarea::placeholder{color:var(--app-text-muted)}
.input-actions{display:flex;justify-content:space-between;align-items:center;gap:8px;padding:0 8px 8px}.model-container{position:relative;min-width:0}.model-toggle{display:flex;gap:5px;align-items:center;max-width:230px;height:28px;padding:5px 7px}.model-toggle span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.send{flex-shrink:0;background:var(--color-primary);color:var(--color-primary-foreground);border-radius:7px}.send:hover{background:var(--color-primary);filter:brightness(1.1)}
.conversation-usage{display:flex;align-items:center;flex-wrap:wrap;gap:8px;font-size:10px;color:var(--app-text-muted);margin-top:8px;line-height:1.5}.cache-rate{border-left:1px solid var(--app-border-light);padding-left:8px}
.history{max-height:190px;overflow:auto;padding:6px 10px;border-bottom:1px solid var(--app-border-light)}.history-title{display:block;width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;text-align:left;padding:6px;border-radius:5px}.history-title:hover,.model-list button:hover{background:var(--app-btn-hover)}
.selected{background:color-mix(in srgb,var(--color-primary) 12%,transparent)}
.model-picker{position:absolute;bottom:calc(100% + 12px);left:0;width:min(310px,calc(100vw - 80px));padding:6px;background:var(--app-bg-dialog);border:1px solid var(--app-border-light);border-radius:8px;box-shadow:var(--niri-shadow-dialog);z-index:20}.model-list{max-height:300px;overflow:auto}.model-list p{font-size:10px;color:var(--app-text-muted);padding:7px 8px}.model-list button{display:flex;align-items:center;justify-content:space-between;gap:8px;width:100%;text-align:left;padding:8px;border-radius:5px;overflow-wrap:anywhere}.model-list button span{min-width:0}.model-list svg{flex-shrink:0;color:var(--color-primary)}.model-list small{display:block;color:var(--app-text-muted)}
.muted{font-size:11px;color:var(--app-text-muted);line-height:1.6}.error{margin:4px 12px;color:var(--color-destructive,#e16b6b);font-size:11px;max-height:64px;overflow:auto}.background-status{padding:7px 12px;background:var(--app-btn-hover);font-size:11px;display:flex;align-items:center;gap:10px}
</style>
