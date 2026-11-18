<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { Maximize2, Minimize2, Plus, X, ArrowUp, Square, ChevronDown, Check } from '@lucide/vue';
import IconButton from '@/components/common/IconButton.vue';
import { Button } from '@/components/ui/button';
import TooltipHint from '@/components/ui/tooltip/TooltipHint.vue';
import { conversationUsage } from '@/utils/aiUsage';
import { formatTokens, modelOptions, modelRecipient, sameModelRecipient, sshApprovalOptions } from '@/utils/ai';
import { aiInvoke, useAiStore } from '@/stores/ai';
import { useSshStore } from '@/stores/ssh';
import { supportsTerminalTextContext } from '@/utils/aiContext';
import { getTerminalContextReader } from '@/utils/terminalContextReaders';
import { DropdownMenu, DropdownMenuItem } from '@/components/ui/dropdown-menu';
import { confirm } from '@/composables/useConfirm';
import AiMessage from './AiMessage.vue';
import AiHistoryPopover from './AiHistoryPopover.vue';

const props = defineProps({ maximized: Boolean, visible: { type: Boolean, default: true }, terminalSessionId: { type: String, default: '' } });
defineEmits(['close', 'maximize']);
const store = useAiStore();
const sshStore = useSshStore();
const skillCatalog = ref(null), skillError = ref(''), skillLoading = ref(false), pickedSkill = ref(null);
let catalogRequest;
const terminalSession = computed(() => sshStore.getSession(props.terminalSessionId));
const terminalTarget = computed(() => {
  const session = terminalSession.value;
  if (session?.status !== 'connected') return null;
  const protocol = session.config?.protocol || 'ssh';
  const kind = protocol === 'local' && session.config?.local_profile === 'cmd' ? 'cmd' : protocol;
  if (kind !== 'cmd' && kind !== 'ssh') return null;
  const id = kind === 'cmd' ? session.id : session.workspaceSessionId || session.id;
  const target = sshStore.getSession(id) || session;
  return { id, kind, label: String(target.name || id).slice(0, 80) };
});
const terminalCmdDirectory = computed(() => {
  const target = terminalTarget.value;
  const session = target?.kind === 'cmd' ? sshStore.getSession(target.id) : null;
  return session?.status === 'connected' ? session.cmdCwd || '' : '';
});
const sshApprovalTarget = computed(() => {
  const target = terminalTarget.value;
  return target?.kind === 'ssh' ? target.id : null;
});
const sshApprovalContext = computed(() => sshApprovalTarget.value
  ? JSON.stringify([store.current?.id ?? store.emptyConversationRevision, sshApprovalTarget.value]) : null);
const sshApprovalBinding = ref(null);
let sshApprovalChange;
const sshApprovalLabel = computed(() => sshApprovalBinding.value?.context === sshApprovalContext.value
  ? sshApprovalOptions.find(option => option.value === sshApprovalBinding.value?.mode)?.label : null);
function bindSshApproval(mode = null) {
  const context = sshApprovalContext.value;
  const binding = { context, pending: true, mode: sshApprovalBinding.value?.context === context ? sshApprovalBinding.value.mode : null };
  sshApprovalBinding.value = binding;
  const request = store.bindSshApproval(sshApprovalTarget.value, mode).then(approval => {
    if (sshApprovalBinding.value?.context === context && sshApprovalChange === request) {
      sshApprovalBinding.value = { context, pending: false, mode: approval?.mode };
    }
    return approval;
  }, error => {
    if (sshApprovalBinding.value?.context === context && sshApprovalChange === request) {
      sshApprovalBinding.value = { context, pending: false, mode: binding.mode };
    }
    throw error;
  });
  sshApprovalChange = request;
  return request;
}
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
watch(historyOpen, open => { if (open) { pickerOpen.value = false; captureMenuOpen.value = false; } });
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
const workspaceName = computed(() => store.workspaces.find(workspace => workspace.id === store.workspaceId)?.name);
const scroller = ref(null);
const messageContent = ref(null);
const followBottom = ref(true);
let readingAnchor = null;
let lastScrollTop = 0;
let scrollGestureUntil = 0;
const options = computed(() => modelOptions(store.settings));
const usage = computed(() => conversationUsage(store.current));
const draft = computed({ get: () => store.current?.draft ?? emptyDraft.value, set: text => {
  if (store.current) store.current.draft = text; else emptyDraft.value = text;
} });
const skillPrefix = /^\s*@([a-z0-9][a-z0-9-]*)(?=\s|$)/;
const skillQuery = computed(() => /^\s*@([a-z0-9-]*)$/.exec(draft.value)?.[1]);
const showSkills = computed(() => !!terminalTarget.value && skillQuery.value !== undefined && !store.active);
const skillKind = computed(() => terminalTarget.value?.kind);
const skillMatches = computed(() => (skillCatalog.value?.skills || [])
  .filter(skill => skill.targetKind === skillKind.value && skill.id.startsWith(skillQuery.value || '')));
const attachedSkill = computed(() => {
  const id = skillPrefix.exec(draft.value)?.[1];
  return pickedSkill.value?.id === id ? pickedSkill.value : null;
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
watch(() => [store.current?.id, terminalTarget.value?.id], () => { pickedSkill.value = null; });
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
const verificationMessages = computed(() => {
  const latest = new Map();
  for (const message of store.current?.messages || []) {
    if (message.task && !message.task.analysisOnly && message.status !== 'streaming') {
      latest.set(message.task.id, message);
    }
  }
  return new Set([...latest.values()].filter(message => message.task.unknownSteps?.length).map(message => message.id));
});
function firstVisibleMessage() {
  const viewport = scroller.value?.getBoundingClientRect();
  if (!viewport?.height || !messageContent.value) return null;
  for (const message of messageContent.value.children) {
    if (!message.dataset.aiMessageId) continue;
    const box = message.getBoundingClientRect();
    if (box.bottom > viewport.top && box.top < viewport.bottom) {
      return { id: message.dataset.aiMessageId, offset: box.top - viewport.top };
    }
  }
  return null;
}

async function showOlder() {
  readingAnchor = firstVisibleMessage();
  followBottom.value = false;
  visibleTurns.value += 30;
  await nextTick();
  syncScrollPosition();
}
watch(() => store.current?.id, () => {
  readingAnchor = null;
  visibleTurns.value = 30; followBottom.value = true;
});
const generatingHere = computed(() => !!store.active && (
  store.active.conversationId === store.current?.id || (!store.current && store.active.pendingInComposer)
));
const draftWrites = new Map();
const draftTimers = new Map();
const pendingDrafts = new Map();
const sending = ref(false);
const verifying = ref(false);
watch([sshApprovalContext, () => props.visible, () => !!store.active, sending], ([context, visible, active, submitting]) => {
  if (!context) {
    sshApprovalBinding.value = null;
  } else if (visible && !active && !submitting && sshApprovalBinding.value?.context !== context) {
    void safe(() => bindSshApproval());
  }
}, { immediate: true });
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
async function newConversation(workspaceId = store.workspaceId) {
  if (creating.value) return;
  const revision = ++navigation, origin = document.activeElement;
  creating.value = true;
  try {
    await flushDrafts();
    if (revision !== navigation) return;
    if (store.newConversation(workspaceId)) {
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
async function verifyDiagnosis(message) {
  if (store.active || sending.value || verifying.value || !verificationMessages.value.has(message.id)) return;
  const conversationId = store.current?.id;
  const task = message.task;
  const operations = task.unknownSteps.map(step => step.command).join('、');
  const steps = task.unknownSteps.map(({ messageId, callId }) => ({ messageId, callId }));
  verifying.value = true;
  try {
    try {
      await confirm({ title: '核实结果', content: `请先在「${task.target.label}」核对以下操作的实际结果。确认后会解除这些步骤的待核实限制。${operations}`, okText: '确认已核实', danger: true });
    } catch { return; }
    if (store.current?.id !== conversationId || store.active || !verificationMessages.value.has(message.id)) return;
    await store.verifyTaskResults(message.id, steps);
  } finally { verifying.value = false; }
}

async function send(regenerateId = null) {
  const analyzeMessageId = regenerateId && store.current?.messages.find(message => message.id === regenerateId)?.task ? regenerateId : null;
  if (sending.value || verifying.value || store.importingText || store.active || !store.selectedModel || (!regenerateId && !draft.value.trim())) return;
  sending.value = true;
  const sourceId = store.current?.id;
  const sourceNavigation = navigation;
  const origin = document.activeElement;
  try {
    const selectedId = store.selectedModelId;
    const target = terminalTarget.value;
    const service = store.selectedModel.service;
    const recipient = modelRecipient(service);
    const confirmHistoryRecipient = !!store.current?.messages.length && (store.current.lastServiceId !== service.id
      || !sameModelRecipient(store.current.lastRecipient, recipient));
    if (confirmHistoryRecipient) {
      const protocol = recipient.protocol === 'anthropic_messages' ? 'Anthropic Messages' : 'OpenAI Chat Completions';
      try { await confirm({ title: `发送到 ${service.name}？`, content: `接收端为 ${recipient.baseUrl}（${protocol}）。本次问题、有效历史对话、摘要及历史工具结果将发送到该接收端。`, okText: '发送' }); } catch { return; }
    }
    const text = draft.value;
    if (!regenerateId && sshApprovalTarget.value) {
      const binding = sshApprovalBinding.value;
      if (binding?.context === sshApprovalContext.value && binding.pending) await sshApprovalChange;
      else if (binding?.context !== sshApprovalContext.value || !binding?.mode) await bindSshApproval();
    }
    let selectedSkill = null;
    if (!regenerateId && target) {
      if (text.trim() === '@') throw new Error('请先选择要使用的 Skill');
      const skillId = skillPrefix.exec(text)?.[1];
      if (skillId) {
        selectedSkill = attachedSkill.value || (await loadSkills()).skills.find(skill => skill.id === skillId);
        if (!selectedSkill || selectedSkill.targetKind !== target.kind) throw new Error(`找不到适用于当前终端的 Skill @${skillId}`);
      }
    }
    await flushDrafts();
    if (navigation !== sourceNavigation || store.current?.id !== sourceId || store.selectedModelId !== selectedId
      || store.selectedModel?.service.id !== service.id || !sameModelRecipient(recipient, modelRecipient(store.selectedModel.service))
      || (!regenerateId && terminalTarget.value !== target)
      || (selectedSkill && draft.value !== text)) return;
    if (!regenerateId) draft.value = '';
    readingAnchor = null;
    followBottom.value = true;
    const taskIntent = { analyzeMessageId, confirmHistoryRecipient };
    if (!regenerateId) {
      taskIntent.terminalTarget = target;
      if (terminalCmdDirectory.value) taskIntent.cmdDirectory = terminalCmdDirectory.value;
      if (selectedSkill) taskIntent.skill = { id: selectedSkill.id, revision: selectedSkill.revision };
    }
    const run = store.send(text, regenerateId, null, null, taskIntent);
    await focusComposer(origin);
    const accepted = await run;
    const answer = store.current?.messages.at(-1);
    if (accepted && selectedSkill && !regenerateId
      && (!sourceId || store.current?.id === sourceId) && answer?.task && !answer.task.analysisOnly) {
      if (answer.status === 'completed' && !answer.tools?.length && !draft.value.trim()) {
        pickedSkill.value = selectedSkill;
        draft.value = `@${selectedSkill.id} `;
      }
    }
    // Validation failures before a persisted user message should retain the input.
    if (!regenerateId && !accepted && navigation === sourceNavigation && store.current?.id === sourceId) draft.value ||= text;
    if (!accepted && selectedSkill && draft.value === text) pickedSkill.value = selectedSkill;
  } finally { sending.value = false; }
}
let scrollFrame = null;
function syncScrollPosition() {
  const viewport = scroller.value;
  if (!props.visible || !viewport?.clientHeight) return;
  if (followBottom.value) {
    viewport.scrollTop = viewport.scrollHeight;
  } else if (readingAnchor) {
    const message = Array.from(messageContent.value.children).find(element => element.dataset.aiMessageId === readingAnchor.id);
    if (message) {
      const offset = message.getBoundingClientRect().top - viewport.getBoundingClientRect().top;
      if (Math.abs(offset - readingAnchor.offset) >= 1) viewport.scrollTop += offset - readingAnchor.offset;
    }
  }
}
function trackScroll() {
  if (!props.visible || !scroller.value) return;
  const { scrollHeight, scrollTop, clientHeight } = scroller.value;
  const movedUp = scrollTop < lastScrollTop;
  lastScrollTop = scrollTop;
  if (readingAnchor) return;
  // Layout changes can emit scroll events as the streaming reply is rendered.
  const nearBottom = scrollHeight - scrollTop - clientHeight < 80;
  if (nearBottom) followBottom.value = true;
  else if (movedUp && performance.now() < scrollGestureUntil) followBottom.value = false;
}
function beginScrollGesture() {
  readingAnchor = null;
  scrollGestureUntil = performance.now() + 500;
}
function scheduleScroll() {
  if (!props.visible || (!followBottom.value && !readingAnchor) || scrollFrame !== null) return;
  scrollFrame = requestAnimationFrame(() => {
    scrollFrame = null;
    syncScrollPosition();
  });
}
watch(() => props.visible, visible => {
  if (!visible && !followBottom.value) readingAnchor = firstVisibleMessage() ?? readingAnchor;
}, { flush: 'sync' });
watch(() => props.visible && [store.current?.id, store.current?.messages.length,
  store.current?.messages.at(-1)?.content, store.current?.messages.at(-1)?.reasoning], scheduleScroll, { flush: 'post' });
let resizeObserver;
onMounted(() => {
  resizeObserver = new ResizeObserver(scheduleScroll);
  if (scroller.value) resizeObserver.observe(scroller.value);
  if (messageContent.value) resizeObserver.observe(messageContent.value);
});
onBeforeUnmount(() => { resizeObserver?.disconnect(); if (scrollFrame !== null) cancelAnimationFrame(scrollFrame); });
onMounted(async () => {
  await safe(() => store.initialize()); loading.value = false;
  if (store.composerRevision) await focusComposer(store.importFocusOrigin());
});
</script>

<template>
  <section class="ai-panel" :class="{ 'is-maximized': maximized }" aria-label="智能助手">
    <header>
      <div class="assistant-heading">
        <strong>智能助手</strong>
        <span v-if="workspaceName" class="assistant-workspace" :title="workspaceName">{{ workspaceName }}</span>
      </div>
      <div class="header-actions">
        <IconButton :icon="Plus" size="sm" :disabled="creating || loading" :aria-busy="creating" aria-label="新建对话" :action="() => safe(newConversation)" />
        <AiHistoryPopover v-model="historyOpen" :disabled="loading"
          @open-conversation="id => safe(() => openConversation(id))"
          @new-conversation="id => safe(() => newConversation(id))" />
        <IconButton :icon="props.maximized ? Minimize2 : Maximize2" size="sm" :aria-label="maximized ? '恢复面板' : '工作区内最大化'" :action="() => $emit('maximize')" />
        <IconButton :icon="X" size="sm" aria-label="关闭面板" :action="() => $emit('close')" />
      </div>
    </header>
    <div v-if="store.active && !generatingHere" class="background-status" role="status"><span>另一个对话正在生成</span><Button variant="ghost" size="sm" :disabled="!store.active.snapshot" @click="safe(() => openConversation(store.active.conversationId))">查看</Button><Button variant="ghost" size="sm" @click="safe(stopGeneration)">停止</Button></div>
    <div ref="scroller" class="messages" @scroll="trackScroll" @wheel.passive="beginScrollGesture"
      @pointerdown="beginScrollGesture" @pointermove="event => { if (event.buttons) beginScrollGesture(); }"
      @keydown="beginScrollGesture">
      <div ref="messageContent" class="message-content">
        <template v-if="visible">
          <Button v-if="turns.length > visibleTurns" variant="ghost" size="sm" @click="safe(showOlder)">加载更早的消息</Button>
          <div v-if="loading" class="empty">正在读取本地对话…</div>
          <div v-else-if="!options.length" class="empty"><strong>配置你的 AI 模型</strong><p>请进入「首选项 → 模型服务」，新增服务并启用模型。</p><p>AI 可独立使用，无需连接 SSH。</p></div>
          <div v-else-if="!messages.length" class="empty"><strong>有什么可以帮你？</strong><p>输入问题，或粘贴代码和报错。</p></div>
          <AiMessage v-for="message in messages" :key="message.id" :data-ai-message-id="message.id" :message="message"
            :request-id="generatingHere && message.status === 'streaming' ? store.active?.requestId : undefined"
            :busy="!!store.active || sending || verifying" :can-verify="verificationMessages.has(message.id)"
            :can-regenerate="message.role === 'assistant' && !message.deployment && message.replyTo === lastUserId && message.status !== 'streaming'"
            @regenerate="safe(() => send(message.id))" @verified="safe(() => verifyDiagnosis(message))"
            @select="safe(() => store.selectAnswer(message.id))" />
          <p v-if="generatingHere && ['context','started'].includes(store.active.stage)" class="muted" role="status">正在准备上下文，长会话可能需要调用模型生成摘要…</p>
        </template>
      </div>
    </div>
    <p v-if="store.error" class="error" role="alert">{{ store.error }}</p>
    <div class="composer">
      <div v-if="terminalTarget" class="read-scope">
        <span>当前终端 · {{ terminalTarget.label }}</span>
        <DropdownMenu v-if="sshApprovalTarget" side="top" align="end" :side-offset="8" hint="自动执行仍会检查命令知识库。切换 SSH 目标后需要重新选择。">
          <template #trigger>
            <Button variant="ghost" size="sm" :aria-label="`当前对话的 SSH 审批模式${sshApprovalLabel ? '，' + sshApprovalLabel : ''}`" :disabled="sending || !!store.active || !!sshApprovalBinding?.pending">{{ sshApprovalLabel || 'SSH 审批' }}<ChevronDown :size="12" /></Button>
          </template>
          <DropdownMenuItem v-for="option in sshApprovalOptions" :key="option.value" @select="safe(() => bindSshApproval(option.value))">
            {{ option.label }}<Check v-if="sshApprovalLabel === option.label" :size="14" />
          </DropdownMenuItem>
        </DropdownMenu>
      </div>
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
        <textarea ref="composer" v-model="draft" aria-label="AI 消息" :placeholder="terminalTarget ? '输入问题或描述要完成的事，输入 @ 可选脚本…' : '输入问题，或粘贴代码和报错…'" rows="3" maxlength="128000" @keydown.enter.exact="if (!$event.isComposing) { $event.preventDefault(); safe(() => send()); }" />
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
          <div class="model-container">
            <DropdownMenu v-model:open="pickerOpen" :id="modelMenuId" side="top" align="end" :side-offset="8" class="w-[min(310px,calc(100vw_-_32px))] max-h-[min(300px,var(--reka-dropdown-menu-content-available-height))]" :hint="store.selectedModel ? `${store.selectedModel.service.name}（${store.selectedModel.modelId}）` : '请选择模型'"
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
          <IconButton v-else :icon="ArrowUp" size="28px" class="send" aria-label="发送消息" tooltip-side="top" :disabled="sending || verifying || store.importingText || !!store.active || !store.selectedModel || !draft.trim()" :action="() => safe(() => send())" />
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
.skill-picker{display:grid;gap:4px;max-height:180px;overflow:auto;margin-bottom:6px;padding:8px;border:1px solid var(--app-border-dark);border-radius:8px;background:var(--app-bg-dialog);font-size:12px;overflow-wrap:anywhere}
.skill-option{justify-content:flex-start;white-space:normal;text-align:left}.skill-directory{user-select:text}
.ai-panel{height:100%;min-height:0;display:flex;flex-direction:column;container-type:size;overflow-y:auto;color:var(--app-text);font-size:12px}
header{display:flex;flex:none;align-items:center;justify-content:space-between;gap:8px;padding:8px 12px;border-bottom:1px solid var(--app-border-light)}.header-actions{display:flex;flex:none;gap:4px}
.messages{flex:1;min-height:28px;overflow:auto;padding:0 12px;scrollbar-gutter:stable}
.message-content{min-height:100%}
.empty{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:10px;min-height:100%;text-align:center;color:var(--app-text-muted);line-height:1.6;padding:24px 8px}.empty strong{color:var(--app-text);font-size:15px}
.composer{position:relative;flex:none;width:100%;padding:8px 12px}
.is-maximized .message-content,.is-maximized .composer{max-width:760px;margin-inline:auto}
.input-shell{border:1px solid var(--app-input-border);border-radius:10px;background:var(--app-input-bg)}.input-shell:focus-within{border-color:var(--app-focus-border)}
textarea{display:block;width:100%;resize:vertical;max-height:min(200px,35cqh);min-height:64px;background:transparent;border:0;padding:8px 10px;color:var(--app-text);font-size:12px;outline:none}textarea::placeholder{color:var(--app-text-muted)}
.input-actions{display:flex;align-items:center;gap:8px;padding:0 8px 8px}.model-container{position:relative;min-width:0;margin-left:auto}.model-toggle{display:flex;gap:5px;align-items:center;max-width:min(230px,100%)}.model-toggle span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.send{flex-shrink:0;background:var(--color-primary);color:var(--color-primary-foreground);border-radius:7px}.send:hover{background:var(--color-primary);filter:brightness(1.1)}
.conversation-usage{display:flex;align-items:center;flex-wrap:wrap;gap:8px;font-size:10px;color:var(--app-text-muted);margin-top:8px;line-height:1.5}.cache-rate{border-left:1px solid var(--app-border-light);padding-left:8px}
.assistant-heading{display:flex;align-items:baseline;gap:8px;min-width:0}.assistant-heading strong{flex-shrink:0}.assistant-workspace{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:var(--app-text-muted);font-size:11px}
.selected{background:color-mix(in srgb,var(--color-primary) 12%,transparent)}
.model-service-name{font-size:10px;color:var(--app-text-muted);padding:7px 8px}.model-option{justify-content:space-between}.model-option span{min-width:0;overflow-wrap:anywhere}.model-option small{display:block;color:var(--app-text-muted)}

.muted{font-size:11px;color:var(--app-text-muted);line-height:1.6}.error{margin:4px 12px;color:var(--color-destructive,#e16b6b);font-size:11px;max-height:64px;overflow:auto;overflow-wrap:anywhere}.background-status{padding:4px 12px;background:var(--app-btn-hover);font-size:11px;display:flex;align-items:center;gap:6px}
.background-status span{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}

@container (max-height: 360px) {
  header,.background-status,.composer{padding:4px 8px}
  textarea{height:48px;min-height:36px;max-height:25cqh;padding-block:6px}
  .input-actions{padding-bottom:4px}
  .conversation-usage{margin-top:4px}
  .read-scope{margin-bottom:2px}
  .error{margin:2px 8px;max-height:26px}
  .skill-picker{position:absolute;z-index:1;inset-inline:8px;bottom:100%;max-height:min(120px,35cqh);margin-bottom:4px;box-shadow:var(--niri-shadow-dialog)}
}
</style>
