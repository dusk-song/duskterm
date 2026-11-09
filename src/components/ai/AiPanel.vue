<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { Maximize2, Minimize2, Plus, History, X, ArrowUp, Square, ChevronDown, Check } from '@lucide/vue';
import IconButton from '@/components/common/IconButton.vue';
import { Button } from '@/components/ui/button';
import TooltipHint from '@/components/ui/tooltip/TooltipHint.vue';
import { conversationUsage } from '@/utils/aiUsage';
import { formatTokens, modelOptions } from '@/utils/ai';
import { aiInvoke, useAiStore } from '@/stores/ai';
import { useSshStore } from '@/stores/ssh';
import { supportsTerminalTextContext } from '@/utils/aiContext';
import { getTerminalContextReader } from '@/utils/terminalContextReaders';
import { DropdownMenu, DropdownMenuItem } from '@/components/ui/dropdown-menu';
import { confirm } from '@/composables/useConfirm';
import AiMessage from './AiMessage.vue';
import AiReadScope from './AiReadScope.vue';

const props = defineProps({ maximized: Boolean, visible: { type: Boolean, default: true }, terminalSessionId: { type: String, default: '' } });
defineEmits(['close', 'maximize']);
const store = useAiStore();
const sshStore = useSshStore();
const readScope = ref(null), readGrant = ref(null), diagnosisTarget = ref(null);
const skillCatalog = ref(null), skillError = ref(''), skillLoading = ref(false), pickedSkill = ref(null);
let catalogRequest;
const terminalSession = computed(() => sshStore.getSession(props.terminalSessionId));
const diagnosisCmdDirectory = computed(() => {
  const intent = diagnosisTarget.value;
  const session = intent?.kind === 'cmd' ? sshStore.getSession(intent.rebindSessionId || intent.id) : null;
  return session?.status === 'connected' ? session.cmdCwd || '' : '';
});
const intentLabel = computed(() => {
  if (diagnosisTarget.value?.kind === 'deployment') return 'SSH 部署计划';
  if (diagnosisTarget.value?.terminalTask) return diagnosisTarget.value.kind === 'cmd' ? '本地 CMD 终端任务' : 'SSH 终端任务';
  if (diagnosisTarget.value?.kind === 'cmd') return '本地 CMD 诊断';
  return 'SSH 诊断';
});
const canCapture = computed(() => !!terminalSession.value && supportsTerminalTextContext(terminalSession.value.config?.protocol));
const captureMenuOpen = ref(false);
let captureController;
let captureSelected = false;
function cancelCapture() { captureController?.abort(); store.cancelTerminalTextImport(); }
function closeCaptureFocus(event) {
  if (captureSelected) { event.preventDefault(); captureSelected = false; }
}
async function captureOutput(mode, lines = 200) {
  captureSelected = true;
  captureMenuOpen.value = false;
  if (!props.visible || !canCapture.value || store.importingText) return;
  const origin = document.activeElement;
  const targetId = props.terminalSessionId;
  const controller = new AbortController();
  captureController = controller;
  try {
    const reader = getTerminalContextReader(targetId);
    await store.importTerminalText(() => reader({ mode, lines, signal: controller.signal }), origin, controller.signal);
    await focusComposer(origin);
  } catch (error) {
    if (error && error.name !== 'AbortError') store.error = String(error.message || error);
  } finally { captureController = null; }
}
watch(() => [props.terminalSessionId, terminalSession.value?.status, terminalSession.value?.config?.protocol, props.visible], () => {
  captureController?.abort(); captureMenuOpen.value = false;
  if (!props.visible && store.importingText) store.cancelTerminalTextImport();
});
onBeforeUnmount(() => captureController?.abort());
watch(() => [props.terminalSessionId, terminalSession.value?.status, props.visible, store.selectedModelId], () => {
  const pending = store.active?.snapshot?.messages.some(message => message.tools?.some(tool => ['waiting_approval', 'approved'].includes(tool.status)));
  if (pending && !store.active?.snapshot?.messages.at(-1)?.task) void store.stop().catch(error => { store.error = String(error); });
});

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
const modelMenuId = `ai-model-menu-${crypto.randomUUID()}`;
let modelSelected = false;
function closeModelFocus(event) {
  if (modelSelected) { event.preventDefault(); modelSelected = false; }
}
function selectModel(id) {
  const origin = document.activeElement;
  modelSelected = true;
  store.selectedModelId = id; pickerOpen.value = false;
  void focusComposer(origin);
}
watch(() => props.visible, visible => {
  if (!visible) { pickerOpen.value = false; historyOpen.value = false; navigation++; }
});
watch(captureMenuOpen, open => { if (open) { pickerOpen.value = false; historyOpen.value = false; } });
watch(pickerOpen, async open => {
  if (!open) return;
  captureMenuOpen.value = false; historyOpen.value = false;
  await nextTick();
  requestAnimationFrame(() => {
    if (disposed || !props.visible || !pickerOpen.value) return;
    const menu = document.getElementById(modelMenuId);
    if (menu?.contains(document.activeElement)) menu.querySelector('[data-current="true"]')?.focus({ preventScroll: true });
  });
});
const emptyDraft = computed({ get: () => store.emptyDraft, set: text => { store.emptyDraft = text; } });
const loading = ref(true);
const scroller = ref(null);
const followBottom = ref(true);
const options = computed(() => modelOptions(store.settings));
const usage = computed(() => conversationUsage(store.current));
const draft = computed({ get: () => store.current?.draft ?? emptyDraft.value, set: text => {
  if (store.current) store.current.draft = text; else emptyDraft.value = text;
} });
const skillPrefix = /^\s*@([a-z0-9][a-z0-9-]*)(?=\s|$)/;
const skillQuery = computed(() => /^\s*@([a-z0-9-]*)$/.exec(draft.value)?.[1]);
const showSkills = computed(() => !!diagnosisTarget.value?.terminalTask && skillQuery.value !== undefined && !store.active);
const skillKind = computed(() => diagnosisTarget.value?.kind || 'ssh');
const skillMatches = computed(() => (skillCatalog.value?.skills || [])
  .filter(skill => skill.targetKind === skillKind.value && skill.id.startsWith(skillQuery.value || '')));
const attachedSkill = computed(() => {
  const id = skillPrefix.exec(draft.value)?.[1];
  return diagnosisTarget.value?.terminalTask && pickedSkill.value?.id === id ? pickedSkill.value : null;
});
function loadSkills() {
  if (!catalogRequest) {
    skillError.value = '';
    skillLoading.value = true;
    catalogRequest = aiInvoke('ai_list_skills').then(catalog => { skillCatalog.value = catalog; return catalog; })
      .catch(error => { skillError.value = String(error); throw error; })
      .finally(() => { skillLoading.value = false; catalogRequest = null; });
  }
  return catalogRequest;
}
watch(showSkills, show => { if (show) void loadSkills().catch(() => {}); });
watch(() => [store.current?.id, diagnosisTarget.value?.id], () => { pickedSkill.value = null; });
watch(() => skillPrefix.exec(draft.value)?.[1], id => { if (pickedSkill.value?.id !== id) pickedSkill.value = null; });
function pickSkill(skill) {
  if (!showSkills.value || skill.targetKind !== skillKind.value) return;
  const origin = document.activeElement;
  pickedSkill.value = skill;
  draft.value = `@${skill.id} `;
  void focusComposer(origin);
}
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
  if (previous && id !== previous) diagnosisTarget.value = null;
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
}, { immediate: true });
watch(() => store.composerRevision, () => {
  historyOpen.value = false; pickerOpen.value = false;
  void focusComposer(store.importFocusOrigin());
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
async function continueDiagnosis(message) {
  if (store.active || sending.value) return;
  const conversationId = store.current?.id;
  const source = store.current?.messages.findLast(item => item.task?.id === message.task.id && !item.task.analysisOnly);
  if (!source) return;
  const task = source.task;
  const kind = task.targetKind || 'ssh';
  let rebindSessionId = null;
  if (sshStore.getSession(task.target.id)?.status !== 'connected') {
    const current = terminalSession.value;
    const session = kind === 'cmd' ? current : sshStore.getSession(current?.workspaceSessionId || current?.id);
    const config = session?.config;
    const matchesKind = kind === 'cmd'
      ? config?.protocol === 'local' && config.local_profile === 'cmd'
      : !!config && ['', 'ssh'].includes(config.protocol || '');
    if (session?.status !== 'connected' || !matchesKind) {
      throw new Error(`请先打开原任务对应的${kind === 'cmd' ? '本地 CMD' : ' SSH'}终端，再继续检查。`);
    }
    const target = kind === 'cmd' ? session.name : `${config.username || ''}@${config.host || session.name}:${config.port || 22}`;
    const details = task.targetIdentity ? '应用会核对原任务的连接信息。' : '旧记录缺少可核对的连接信息，请确认这是原来的目标。';
    try {
      await confirm({ title: '确认续查连接', content: `原目标「${task.target.label}」，当前连接「${target}」。${details}`, okText: '使用此连接' });
    } catch { return; }
    if (store.current?.id !== conversationId || store.active || sending.value) return;
    rebindSessionId = session.id;
  }
  readScope.value?.clear();
  diagnosisTarget.value = { id: task.target.id, label: task.target.label, kind, terminalTask: task.terminalTask, messageId: source.id, rebindSessionId };
  if (!draft.value.trim()) draft.value = task.terminalTask ? '请根据已有结果继续完成任务。' : '请根据已有证据继续检查。';
  await focusComposer();
}

async function send(regenerateId = null, analyzeMessageId = null) {
  if (regenerateId && store.current?.messages.find(message => message.id === regenerateId)?.task) analyzeMessageId = regenerateId;
  if (sending.value || store.importingText || store.active || !store.selectedModel || (!regenerateId && !analyzeMessageId && !draft.value.trim())) return;
  sending.value = true;
  const sourceId = store.current?.id;
  const origin = document.activeElement;
  try {
    const selectedId = store.selectedModelId;
    const intent = diagnosisTarget.value;
    const service = store.selectedModel.service;
    const oldService = store.current?.lastServiceId;
    if (oldService && oldService !== service.id) {
      try { await confirm({ title: `发送到 ${service.name}？`, content: '本次问题、有效历史对话、摘要及历史工具读取结果将发送到该服务。切换服务不会删除本地历史。', okText: '发送' }); } catch { return; }
    }
    const text = analyzeMessageId ? '继续分析已保存的诊断证据，不执行任何操作。' : draft.value;
    let selectedSkill = null;
    if (intent?.terminalTask && !regenerateId && !analyzeMessageId) {
      if (text.trim() === '@') throw new Error('请先选择要使用的 Skill');
      const skillId = skillPrefix.exec(text)?.[1];
      if (skillId) {
        selectedSkill = attachedSkill.value || (await loadSkills()).skills.find(skill => skill.id === skillId);
        if (!selectedSkill || selectedSkill.targetKind !== (intent.kind || 'ssh')) throw new Error(`找不到适用于当前终端的 Skill @${skillId}`);
      }
    }
    await flushDrafts();
    if (store.current?.id !== sourceId || store.selectedModelId !== selectedId
      || (!analyzeMessageId && diagnosisTarget.value !== intent) || (selectedSkill && draft.value !== text)) return;
    const grant = !analyzeMessageId && !intent ? readScope.value?.consume() : null;
    if (!regenerateId && !analyzeMessageId) draft.value = '';
    followBottom.value = true;
    const taskIntent = { analyzeMessageId };
    if (!regenerateId && !analyzeMessageId && intent) {
      if (diagnosisCmdDirectory.value) taskIntent.cmdDirectory = diagnosisCmdDirectory.value;
      if (intent.kind === 'deployment') {
        taskIntent.deploymentTarget = { id: intent.id, label: intent.label };
      } else if (selectedSkill) {
        taskIntent.diagnosisTarget = { id: intent.rebindSessionId || intent.id, label: intent.label, kind: intent.kind || 'ssh', terminalTask: true };
        taskIntent.skill = { id: selectedSkill.id, revision: selectedSkill.revision };
      } else if (intent.messageId) {
        taskIntent.continueDiagnosis = { messageId: intent.messageId, rebindSessionId: intent.rebindSessionId || null };
      } else {
        taskIntent.diagnosisTarget = intent;
      }
    }
    const run = store.send(text, regenerateId, grant?.readGrant || null, grant?.execGrant || null, taskIntent);
    await focusComposer(origin);
    const accepted = await run;
    const answer = store.current?.messages.at(-1);
    if (accepted && !regenerateId && !analyzeMessageId && intent && diagnosisTarget.value === intent
      && (!sourceId || store.current?.id === sourceId) && answer?.task && !answer.task.analysisOnly) {
      const { target, targetKind, terminalTask } = answer.task;
      if (selectedSkill) {
        const awaitingInputs = answer.status === 'completed' && !answer.tools?.length;
        if (awaitingInputs && !draft.value.trim()) {
          pickedSkill.value = selectedSkill;
          draft.value = `@${selectedSkill.id} `;
        } else if (!awaitingInputs) {
          diagnosisTarget.value = null;
        }
      } else {
        diagnosisTarget.value = { id: target.id, label: target.label, kind: targetKind || intent.kind || 'ssh', terminalTask, messageId: answer.id };
      }
    }
    // Validation failures before a persisted user message should retain the input.
    if (!regenerateId && !analyzeMessageId && !accepted && store.current?.id === sourceId) draft.value ||= text;
    if (!regenerateId && !analyzeMessageId && !accepted && !sourceId && !store.current) emptyDraft.value ||= text;
    if (!accepted && selectedSkill && diagnosisTarget.value === intent && draft.value === text) pickedSkill.value = selectedSkill;
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
onMounted(async () => {
  await safe(() => store.initialize()); loading.value = false;
  if (store.composerRevision) await focusComposer(store.importFocusOrigin());
});
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
      <AiMessage v-for="message in messages" :key="message.id" :message="message" :request-id="generatingHere ? store.active?.requestId : undefined" :busy="!!store.active" :can-regenerate="message.role === 'assistant' && !message.deployment && message.replyTo === lastUserId && message.status !== 'streaming'" @regenerate="safe(() => send(message.id))" @analyze="safe(() => send(null, message.id))" @continue="safe(() => continueDiagnosis(message))" @select="safe(() => store.selectAnswer(message.id))" />
      <p v-if="generatingHere && ['context','started'].includes(store.active.stage)" class="muted" role="status">正在准备上下文，长会话可能需要调用模型生成摘要…</p>
      </div>
    </div>
    <p v-if="store.error" class="error" role="alert">{{ store.error }}</p>
    <div class="composer">
      <div v-if="diagnosisTarget" class="read-scope" :aria-label="`${intentLabel}模式`"><span :title="diagnosisTarget.id">{{ intentLabel }}：{{ diagnosisTarget.label }} · {{ diagnosisTarget.kind === 'deployment' ? '预览待审阅步骤' : diagnosisTarget.messageId ? '继续同一任务' : diagnosisTarget.terminalTask ? '目录操作自动执行，Shell 命令需确认' : '按需提出检查命令' }}</span><Button variant="ghost" size="sm" @click="diagnosisTarget = null">退出</Button></div>
      <p v-if="diagnosisTarget?.kind === 'cmd'" class="muted" :title="diagnosisCmdDirectory">
        {{ diagnosisCmdDirectory ? `终端上报目录：${diagnosisCmdDirectory}` : '当前目录未知，依赖目录的检查需要先确认路径。' }}
      </p>
      <div v-if="readGrant" class="read-scope"><span :title="readGrant.root">本次工具：{{ terminalSession?.name || readGrant.target }} · {{ readGrant.root || '命令需逐条确认' }}{{ readGrant.root && readGrant.execToken ? ' · 命令需确认' : '' }}</span><Button variant="ghost" size="sm" @click="readScope?.clear()">取消</Button></div>
      <div v-if="attachedSkill" class="read-scope" aria-label="已选 Skill">
        <span :title="attachedSkill.revision">@{{ attachedSkill.id }} · {{ attachedSkill.version }} · {{ attachedSkill.directory }}</span>
        <Button variant="ghost" size="sm" @click="draft = draft.replace(skillPrefix, '').trimStart()">移除 Skill</Button>
      </div>
      <section v-if="showSkills" class="skill-picker" aria-label="选择本地 Skill">
        <strong>选择本地 Skill</strong>
        <p v-if="skillLoading">正在读取 Skill…</p>
        <p v-else-if="skillError" role="alert">{{ skillError }}</p>
        <template v-else>
          <Button v-for="skill in skillMatches" :key="skill.id" variant="ghost" size="sm" class="skill-option" :title="skill.description" @click="pickSkill(skill)">
            @{{ skill.id }} · {{ skill.label }} · {{ skill.version }}
          </Button>
          <p v-if="!skillMatches.length">没有匹配当前终端的 Skill。</p>
          <p v-if="skillCatalog" class="muted skill-directory">{{ skillCatalog.directory }}</p>
          <details v-if="skillCatalog?.issues.length"><summary>配置问题</summary><p v-for="issue in skillCatalog.issues" :key="issue">{{ issue }}</p></details>
        </template>
      </section>
      <div class="input-shell">
        <textarea ref="composer" v-model="draft" aria-label="AI 消息" :placeholder="diagnosisTarget?.terminalTask ? '描述任务，或输入 @ 选择固定脚本…' : '输入问题，或粘贴代码和报错…'" rows="3" maxlength="128000" @keydown.enter.exact="if (!$event.isComposing) { $event.preventDefault(); safe(() => send()); }" />
        <div class="input-actions">
          <Button v-if="store.importingText" variant="ghost" size="sm" @click="cancelCapture">取消带入</Button>
          <DropdownMenu v-model:open="captureMenuOpen" side="top" :side-offset="8" @close-auto-focus="closeCaptureFocus">
            <template #trigger>
              <Button v-show="!store.importingText" variant="ghost" size="sm" :disabled="loading || !canCapture || store.importingText" title="从当前活动终端带入文字，可编辑后发送">带入终端输出<ChevronDown :size="12" /></Button>
            </template>
            <DropdownMenuItem @select="captureOutput('visible')">当前可见区域</DropdownMenuItem>
            <DropdownMenuItem @select="captureOutput('recent')">最近 200 行</DropdownMenuItem>
            <DropdownMenuItem @select="captureOutput('recent', 500)">最近 500 行</DropdownMenuItem>
            <DropdownMenuItem @select="captureOutput('selection')">当前选区</DropdownMenuItem>
          </DropdownMenu>
          <AiReadScope ref="readScope" :session="terminalSession" :model-id="store.selectedModelId" :service-name="store.selectedModel?.service.name" :conversation-id="store.current?.id"
            :visible="props.visible" :busy="!!store.active || sending || loading" :diagnosis-target="diagnosisTarget" @diagnose="diagnosisTarget = $event" @change="readGrant = $event" @focus="focusComposer()" />
          <div class="model-container">
            <DropdownMenu v-model:open="pickerOpen" :id="modelMenuId" side="top" align="end" :side-offset="8" class="model-picker" :hint="store.selectedModel ? `${store.selectedModel.service.name}（${store.selectedModel.modelId}）` : '请选择模型'"
              @close-auto-focus="closeModelFocus">
              <template #trigger>
                <Button variant="ghost" size="sm" class="model-toggle" :disabled="loading || !options.length"><span>{{ store.selectedModel?.name || '选择模型' }}</span><ChevronDown :size="12" /></Button>
              </template>
              <template v-for="service in options" :key="service.id">
                <p class="model-service-name">{{ service.name }}</p>
                <DropdownMenuItem v-for="model in service.models" :key="model.id" class="model-option" :class="{ selected: store.selectedModelId === model.id }"
                  :data-current="store.selectedModelId === model.id" :text-value="model.name" @select="selectModel(model.id)">
                  <span>{{ model.name }}<small v-if="model.name !== model.modelId">{{ model.modelId }}</small></span><Check v-if="store.selectedModelId === model.id" :size="14" />
                </DropdownMenuItem>
              </template>
            </DropdownMenu>
          </div>
          <IconButton v-if="generatingHere" :icon="Square" size="28px" class="send stop" aria-label="停止生成" tooltip-side="top" :action="() => safe(stopGeneration)" />
          <IconButton v-else :icon="ArrowUp" size="28px" class="send" aria-label="发送消息" tooltip-side="top" :disabled="sending || store.importingText || !!store.active || !store.selectedModel || !draft.trim()" :action="() => safe(() => send())" />
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
.read-scope{display:flex;align-items:center;gap:6px;min-width:0;font-size:11px;color:var(--app-text-muted);margin-bottom:6px}.read-scope>span{flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.skill-picker{display:grid;gap:4px;max-height:180px;overflow:auto;margin-bottom:6px;padding:8px;border:1px solid var(--app-border-light);border-radius:8px;font-size:12px;overflow-wrap:anywhere}
.skill-option{justify-content:flex-start;white-space:normal;text-align:left}.skill-directory{user-select:text}
.ai-panel{height:100%;min-height:0;display:flex;flex-direction:column;color:var(--app-text);font-size:12px;background:var(--app-panel-bg)}
header{display:flex;align-items:center;justify-content:space-between;padding:10px 12px;border-bottom:1px solid var(--app-border-light)}.header-actions{display:flex;gap:4px}
.messages{flex:1;min-height:0;overflow:auto;padding:0 14px;scrollbar-gutter:stable}
.message-content{min-height:100%}
.empty{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:10px;min-height:100%;text-align:center;color:var(--app-text-muted);line-height:1.6;padding:24px 8px}.empty strong{color:var(--app-text);font-size:15px}
.composer{padding:10px 12px 8px}.input-shell{border:1px solid var(--app-input-border);border-radius:10px;background:var(--app-input-bg)}.input-shell:focus-within{border-color:var(--app-focus-border)}
textarea{width:100%;resize:vertical;max-height:200px;min-height:65px;background:transparent;border:0;padding:10px;color:var(--app-text);font-size:12px;outline:none}textarea::placeholder{color:var(--app-text-muted)}
.input-actions{display:flex;align-items:center;gap:8px;padding:0 8px 8px}.model-container{position:relative;min-width:0;margin-left:auto}.model-toggle{display:flex;gap:5px;align-items:center;max-width:min(230px,100%)}.model-toggle span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.send{flex-shrink:0;background:var(--color-primary);color:var(--color-primary-foreground);border-radius:7px}.send:hover{background:var(--color-primary);filter:brightness(1.1)}
.conversation-usage{display:flex;align-items:center;flex-wrap:wrap;gap:8px;font-size:10px;color:var(--app-text-muted);margin-top:8px;line-height:1.5}.cache-rate{border-left:1px solid var(--app-border-light);padding-left:8px}
.history{max-height:190px;overflow:auto;padding:6px 10px;border-bottom:1px solid var(--app-border-light)}.history-title{display:block;width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;text-align:left;padding:6px;border-radius:5px}.history-title:hover{background:var(--app-btn-hover)}
.selected{background:color-mix(in srgb,var(--color-primary) 12%,transparent)}
.model-picker{width:min(310px,calc(100vw - 32px));max-height:300px}.model-service-name{font-size:10px;color:var(--app-text-muted);padding:7px 8px}.model-option{justify-content:space-between}.model-option span{min-width:0;overflow-wrap:anywhere}.model-option small{display:block;color:var(--app-text-muted)}

.muted{font-size:11px;color:var(--app-text-muted);line-height:1.6}.error{margin:4px 12px;color:var(--color-destructive,#e16b6b);font-size:11px;max-height:64px;overflow:auto}.background-status{padding:7px 12px;background:var(--app-btn-hover);font-size:11px;display:flex;align-items:center;gap:10px}
</style>
