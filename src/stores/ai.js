import { defineStore } from 'pinia';
import { Channel, invoke } from '@tauri-apps/api/core';
import { acceptsAiEvent } from '@/utils/ai';
import { appendTerminalContext } from '@/utils/aiContext';

// Keep credentials and provider errors out of the generic IPC logger.
export const aiInvoke = (command, args = {}) => invoke(command, args);
const sessions = new WeakMap();
function session(store) {
  if (!sessions.has(store)) sessions.set(store, { view: 0, list: 0, initializing: null, run: null });
  return sessions.get(store);
}

export const useAiStore = defineStore('ai', {
  state: () => ({ settings: { services: [], defaultModelId: null }, conversations: [], current: null,
    active: null, error: '', initialized: false, selectedModelId: null, emptyDraft: '', importingText: false, composerRevision: 0 }),
  getters: {
    selectedModel(state) {
      for (const service of state.settings.services) {
        const model = service.models.find(m => m.id === state.selectedModelId && m.enabled);
        if (model) return { ...model, service };
      }
      return null;
    },
  },
  actions: {
    async initialize({ selectConversation = true } = {}) {
      if (this.initialized) return;
      const local = session(this);
      if (!local.initializing) {
        const view = local.view;
        local.initializing = (async () => {
          await Promise.all([this.loadSettings(), this.loadList()]);
          if (selectConversation && !this.importingText && local.view === view && !this.current && this.conversations.length) await this.open(this.conversations[0].id);
          this.initialized = true;
        })().finally(() => { local.initializing = null; });
      }
      return local.initializing;
    },
    importFocusOrigin() { return session(this).focusOrigin; },
    cancelTerminalTextImport() { session(this).importCancelled = true; },
    async importTerminalText(context, focusOrigin = null, signal = null) {
      if (this.importingText) throw new Error('正在带入文本，请稍候');
      if (this.active?.stage === 'preparing') throw new Error('正在准备发送，请稍后再带入选区');
      this.error = '';
      this.importingText = true;
      const local = session(this), view = local.view;
      local.importCancelled = false;
      const sourceId = this.current?.id;
      const original = this.current?.draft ?? this.emptyDraft;
      const unchanged = () => local.view === view && this.current?.id === sourceId
        && (this.current?.draft ?? this.emptyDraft) === original;
      const check = () => {
        if (signal?.aborted || local.importCancelled) throw new DOMException('带入已取消', 'AbortError');
        if (!unchanged()) throw new Error('对话或草稿已变化，请重新带入终端文本');
      };
      try {
        await this.initialize({ selectConversation: false });
        check();
        context = typeof context === 'function' ? await context() : context;
        check();
        const text = appendTerminalContext(original, context.text);
        if (!this.current) {
          const created = await this.create({ preserveModel: true, canApply: () => !signal?.aborted && !local.importCancelled && this.current?.id === sourceId
            && (this.current?.draft ?? this.emptyDraft) === original });
          if (signal?.aborted || local.importCancelled) throw new DOMException('带入已取消', 'AbortError');
          if (this.current?.id !== created.id || this.current.draft) throw new Error('对话已变化，请重新带入选区');
        }
        this.current.draft = text;
        if (!sourceId) this.emptyDraft = '';
        local.focusOrigin = focusOrigin;
        this.composerRevision++;
        return true;
      } finally { this.importingText = false; }
    },
    async loadSettings() {
      this.settings = await aiInvoke('ai_get_settings');
      if (!this.selectedModel) this.selectedModelId = this.settings.defaultModelId;
    },
    async loadList() {
      const local = session(this), revision = ++local.list;
      const list = await aiInvoke('ai_list_conversations');
      if (revision === local.list) this.conversations = list;
    },
    async open(id) {
      const local = session(this), revision = ++local.view;
      const conversation = await aiInvoke('ai_get_conversation', { id });
      if (revision !== local.view) return false;
      this.current = this.active?.conversationId === id && this.active.snapshot
        ? { ...this.active.snapshot, draft: conversation.draft } : conversation;
      this.selectedModelId = conversation.modelId || this.settings.defaultModelId;
      if (!this.selectedModel) this.selectedModelId = this.settings.defaultModelId;
      this.error = '';
      return true;
    },
    async create({ canApply = () => true, preserveModel = false } = {}) {
      const local = session(this), revision = ++local.view;
      const conversation = await aiInvoke('ai_create_conversation');
      if (revision === local.view && canApply()) {
        this.current = conversation;
        if (!preserveModel) this.selectedModelId = this.settings.defaultModelId;
        this.error = '';
      }
      await this.loadList();
      return conversation;
    },
    async saveDraft(id, text) { await aiInvoke('ai_save_draft', { id, text }); },
    async stop() {
      if (!this.active) return;
      this.active.cancelRequested = true;
      if (this.active.stage !== 'preparing') await aiInvoke('ai_cancel', { requestId: this.active.requestId });
    },
    async suspend() {
      if (this.active) this.active.cancelRequested = true;
      this.cancelTerminalTextImport();
      await aiInvoke('ai_suspend');
    },
    async deleteConversation(id) {
      if (this.active?.conversationId === id) { await this.stop(); await this.waitForIdle(); }
      await aiInvoke('ai_delete_conversation', { id });
      if (this.current?.id === id) { session(this).view++; this.current = null; }
      await this.loadList();
      if (!this.current && this.conversations.length) await this.open(this.conversations[0].id);
    },
    async deleteMessage(id) {
      const conversationId = this.current?.id;
      if (!conversationId) return;
      if (this.active?.conversationId === conversationId) { await this.stop(); await this.waitForIdle(); }
      const conversation = await aiInvoke('ai_delete_message', { conversationId, messageId: id });
      if (this.current?.id === conversationId) this.current = conversation;
    },
    async selectAnswer(id) {
      const conversationId = this.current?.id, revision = session(this).view;
      if (!conversationId) return;
      const conversation = await aiInvoke('ai_select_answer', { conversationId, messageId: id });
      if (session(this).view === revision && this.current?.id === conversationId) this.current = conversation;
    },
    async waitForIdle() { await session(this).run; },
    async send(content, regenerateId = null, readGrant = null, execGrant = null, taskIntent = {}) {
      if (this.active || this.importingText || !this.selectedModel) return false;
      const model = this.selectedModel;
      const local = session(this), view = local.view;
      const requestId = crypto.randomUUID();
      let conversationId = this.current?.id;
      let accepted = false, timer = null;
      const pending = new Map();
      const flush = () => {
        clearTimeout(timer); timer = null;
        if (this.active?.requestId !== requestId) { pending.clear(); return; }
        for (const [id, delta] of pending) {
          const message = this.active.snapshot?.messages.find(m => m.id === id);
          if (message) { message.content += delta.text; message.reasoning += delta.reasoning; }
        }
        pending.clear();
      };
      this.error = '';
      // Reserve before the first await, including creation of the first conversation.
      this.active = { requestId, conversationId, seq: -1, stage: 'preparing', snapshot: null, cancelRequested: false };
      const channel = new Channel();
      channel.onmessage = event => {
        if (!acceptsAiEvent(this.active, event)) return;
        accepted = true;
        this.active.seq = event.seq;
        if (event.kind === 'text' || event.kind === 'reasoning') {
          const message = this.active.snapshot?.messages.find(message => message.id === event.messageId);
          if (message) message.phase = event.kind;
          const delta = pending.get(event.messageId) || { text: '', reasoning: '' };
          delta[event.kind] += event.text || '';
          pending.set(event.messageId, delta);
          if (!timer) timer = setTimeout(flush, 32);
          return;
        }
        flush();
        if (event.task) {
          const message = this.active.snapshot?.messages.find(message => message.id === event.messageId);
          if (message) message.task = event.task;
        }
        if (event.kind === 'task') { this.active.stage = 'streaming'; return; }
        if (event.kind === 'tool' && event.tool) {
          const message = this.active.snapshot?.messages.find(message => message.id === event.messageId);
          if (message) {
            message.phase = 'tool';
            message.tools ||= [];
            const index = message.tools.findIndex(tool => tool.callId === event.tool.callId);
            if (index < 0) message.tools.push(event.tool); else message.tools[index] = event.tool;
          }
          this.active.stage = 'streaming';
          return;
        }
        this.active.stage = event.kind;
        if (event.conversation) {
          this.active.snapshot = event.conversation;
          if (this.current?.id === conversationId) {
            const draft = this.current.draft;
            this.current = event.conversation; this.current.draft = draft;
          }
        }
        // Cancellation while the command was being dispatched must reach Rust too.
        if (event.kind === 'started' && this.active.cancelRequested) void this.stop().catch(e => { this.error = String(e); });
      };
      const run = (async () => {
        try {
          if (!conversationId) {
            const conversation = await aiInvoke('ai_create_conversation');
            conversationId = conversation.id;
            this.active.conversationId = conversationId;
            if (local.view === view) { this.current = conversation; this.selectedModelId = model.id; }
          }
          if (this.active.cancelRequested) return;
          const conversation = await aiInvoke('ai_send', { request: { conversationId, requestId,
            modelId: model.id, content, regenerateId, confirmedServiceId: model.service.id, readGrant, execGrant,
            diagnosisTarget: taskIntent.diagnosisTarget || null, analyzeMessageId: taskIntent.analyzeMessageId || null }, channel });
          flush();
          accepted = true;
          if (this.current?.id === conversationId) {
            const draft = this.current.draft;
            this.current = conversation; this.current.draft = draft;
            this.error = conversation.messages.at(-1)?.error || '';
          }
        } catch (error) {
          flush();
          const message = String(error);
          if (!conversationId || this.current?.id === conversationId) this.error = message;
          const partial = this.active?.snapshot?.messages.at(-1);
          if (partial?.status === 'streaming') {
            partial.status = 'failed'; partial.error = message;
            if (partial.task) partial.task.status = 'failed';
            for (const tool of partial.tools || []) if (['running', 'waiting_approval', 'approved'].includes(tool.status)) {
              tool.approvalId = null; tool.approvalExpiresAt = null;
              const uncertain = tool.execution && ['dispatching', 'running'].includes(tool.execution.outcome);
              tool.status = uncertain ? 'unknown' : 'interrupted';
              if (tool.execution) { tool.execution.outcome = uncertain ? 'unknown' : 'not_started'; tool.execution.outputComplete = false; }
              tool.result = uncertain ? '聊天连接中断，无法确认远端进程是否结束；请先核实状态，不自动重试' : '聊天连接中断，未派发命令或未获取查询结果';
            }
          }
        } finally {
          if (readGrant) await aiInvoke('ai_revoke_read_scope', { token: readGrant }).catch(() => {});
          if (execGrant) await aiInvoke('ai_revoke_exec_scope', { token: execGrant }).catch(() => {});
          flush();
          if (!accepted && !regenerateId && !taskIntent.analyzeMessageId && this.current?.id === conversationId) this.current.draft ||= content;
          if (this.active?.requestId === requestId) this.active = null;
          try { await this.loadList(); } catch (error) { this.error ||= String(error); }
        }
      })();
      local.run = run;
      await run;
      return accepted;
    },
  },
});
