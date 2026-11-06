import { defineStore } from 'pinia';
import { Channel, invoke } from '@tauri-apps/api/core';
import { acceptsAiEvent } from '@/utils/ai';

// Keep credentials and provider errors out of the generic IPC logger.
export const aiInvoke = (command, args = {}) => invoke(command, args);
const sessions = new WeakMap();
function session(store) {
  if (!sessions.has(store)) sessions.set(store, { view: 0, list: 0, initializing: null, run: null });
  return sessions.get(store);
}

export const useAiStore = defineStore('ai', {
  state: () => ({ settings: { services: [], defaultModelId: null }, conversations: [], current: null,
    active: null, error: '', initialized: false, selectedModelId: null }),
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
    async initialize() {
      if (this.initialized) return;
      const local = session(this);
      if (!local.initializing) {
        const view = local.view;
        local.initializing = (async () => {
          await Promise.all([this.loadSettings(), this.loadList()]);
          if (local.view === view && !this.current && this.conversations.length) await this.open(this.conversations[0].id);
          this.initialized = true;
        })().finally(() => { local.initializing = null; });
      }
      return local.initializing;
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
    async create() {
      const local = session(this), revision = ++local.view;
      const conversation = await aiInvoke('ai_create_conversation');
      if (revision === local.view) {
        this.current = conversation;
        this.selectedModelId = this.settings.defaultModelId;
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
    async send(content, regenerateId = null) {
      if (this.active || !this.selectedModel) return false;
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
          const delta = pending.get(event.messageId) || { text: '', reasoning: '' };
          delta[event.kind] += event.text || '';
          pending.set(event.messageId, delta);
          if (!timer) timer = setTimeout(flush, 32);
          return;
        }
        flush();
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
            modelId: model.id, content, regenerateId, confirmedServiceId: model.service.id }, channel });
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
          if (partial?.status === 'streaming') { partial.status = 'failed'; partial.error = message; }
        } finally {
          flush();
          if (!accepted && !regenerateId && this.current?.id === conversationId) this.current.draft ||= content;
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
