import { defineStore } from 'pinia';
import { Channel, invoke } from '@tauri-apps/api/core';
import { acceptsAiEvent } from '@/utils/ai';

// Never pass AI credentials or provider errors through the generic IPC logger.
export const aiInvoke = (command, args = {}) => invoke(command, args);

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
      await Promise.all([this.loadSettings(), this.loadList()]);
      if (this.conversations.length) await this.open(this.conversations[0].id);
      this.initialized = true;
    },
    async loadSettings() {
      this.settings = await aiInvoke('ai_get_settings');
      if (!this.selectedModel) this.selectedModelId = this.settings.defaultModelId;
    },
    async loadList() { this.conversations = await aiInvoke('ai_list_conversations'); },
    async open(id) {
      const conversation = await aiInvoke('ai_get_conversation', { id });
      this.current = this.active?.conversationId === id && this.active.snapshot
        ? { ...this.active.snapshot, draft: conversation.draft } : conversation;
      this.selectedModelId = conversation.modelId || this.settings.defaultModelId;
    },
    async create() {
      this.current = await aiInvoke('ai_create_conversation');
      this.selectedModelId = this.settings.defaultModelId;
      this.error = '';
      await this.loadList();
    },
    async saveDraft(id, text) { await aiInvoke('ai_save_draft', { id, text }); },
    async stop() {
      if (this.active) await aiInvoke('ai_cancel', { requestId: this.active.requestId });
    },
    async deleteConversation(id) {
      if (this.active?.conversationId === id) { await this.stop(); await this.waitForIdle(); }
      await aiInvoke('ai_delete_conversation', { id });
      if (this.current?.id === id) this.current = null;
      await this.loadList();
      if (!this.current && this.conversations.length) await this.open(this.conversations[0].id);
    },
    async deleteMessage(id) {
      if (this.active?.conversationId === this.current?.id) { await this.stop(); await this.waitForIdle(); }
      this.current = await aiInvoke('ai_delete_message', { conversationId: this.current.id, messageId: id });
    },
    async selectAnswer(id) {
      this.current = await aiInvoke('ai_select_answer', { conversationId: this.current.id, messageId: id });
    },
    async waitForIdle() {
      // Await the existing run promise; no polling or new request is involved.
      await this._runPromise;
    },
    async send(content, regenerateId = null) {
      if (this.active || !this.selectedModel) return;
      const selectedId = this.selectedModelId;
      if (!this.current) { await this.create(); this.selectedModelId = selectedId; }
      const model = this.selectedModel;
      if (!model) return;
      let accepted = false;
      const requestId = crypto.randomUUID();
      const conversationId = this.current.id;
      const channel = new Channel();
      this.error = '';
      this.active = { requestId, conversationId, seq: -1, stage: 'preparing', snapshot: null };
      channel.onmessage = event => {
        if (!acceptsAiEvent(this.active, event)) return;
        accepted = true;
        this.active.seq = event.seq;
        this.active.stage = event.kind;
        if (event.conversation) this.active.snapshot = event.conversation;
        if (event.conversation && this.current?.id === conversationId) {
          const draft = this.current.draft;
          this.current = event.conversation;
          this.current.draft = draft;
        }
        if (event.kind === 'text' || event.kind === 'reasoning') {
          const message = this.active.snapshot?.messages.find(m => m.id === event.messageId);
          if (message) { const key = event.kind === 'reasoning' ? 'reasoning' : 'content'; message[key] = (message[key] || '') + (event.text || ''); }
        }
      };
      const run = (async () => {
        try {
          const conversation = await aiInvoke('ai_send', { request: { conversationId, requestId,
            modelId: model.id, content, regenerateId, confirmedServiceId: model.service.id }, channel });
          accepted = true;
          if (this.current?.id === conversationId) {
            const draft = this.current.draft;
            this.current = conversation; this.current.draft = draft;
          }
          if (this.current?.id === conversationId) this.error = conversation.messages.at(-1)?.error || '';
        } catch (error) {
          this.error = String(error);
          const partial = this.active?.snapshot?.messages.at(-1);
          if (partial?.status === 'streaming') { partial.status = 'failed'; partial.error = this.error; }
        }
        finally {
          if (this.active?.requestId === requestId) this.active = null;
          try { await this.loadList(); } catch (error) { this.error ||= String(error); }
        }
      })();
      // Nonreactive Promise is only used to serialize deletion after cancellation.
      Object.defineProperty(this, '_runPromise', { value: run, writable: true, configurable: true });
      await run;
      return accepted;
    },
  },
});
