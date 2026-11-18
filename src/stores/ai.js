import { defineStore } from 'pinia';
import { Channel, invoke } from '@tauri-apps/api/core';
import { acceptsAiEvent, modelRecipient } from '@/utils/ai';
import { appendTerminalContext } from '@/utils/aiContext';

// Keep credentials and provider errors out of the generic IPC logger.
export const aiInvoke = (command, args = {}) => invoke(command, args);
const sessions = new WeakMap();
function session(store) {
  if (!sessions.has(store)) sessions.set(store, { view: 0, list: 0, initializing: null, run: null });
  return sessions.get(store);
}

export const useAiStore = defineStore('ai', {
  state: () => ({ settings: { services: [], defaultModelId: null, defaultSshApprovalMode: 'manual' }, workspaces: [], workspaceId: null, conversations: [], current: null,
    active: null, error: '', initialized: false, selectedModelId: null, emptyDraft: '', importingText: false, composerRevision: 0,
    emptyConversationRevision: 0, draftSshApproval: null }),
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
        local.initializing = (async () => {
          await Promise.all([this.loadSettings(), this.loadList()]);
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
        await this.initialize();
        check();
        context = typeof context === 'function' ? await context() : context;
        check();
        const text = appendTerminalContext(original, context.text);
        if (this.current) this.current.draft = text;
        else this.emptyDraft = text;
        local.focusOrigin = focusOrigin;
        this.composerRevision++;
        return true;
      } finally { this.importingText = false; }
    },
    async loadSettings() {
      this.settings = await aiInvoke('ai_get_settings');
      if (!this.selectedModel) this.selectedModelId = this.settings.defaultModelId;
    },
    bindSshApproval(target, mode = null) {
      const local = session(this), view = local.view;
      const conversationId = this.current?.id ?? null;
      const current = () => local.view === view && (this.current?.id ?? null) === conversationId;
      const operation = (local.sshApprovalChange || Promise.resolve()).catch(() => {}).then(async () => {
        if (!current()) return;
        const draft = conversationId ? null : this.draftSshApproval;
        const approval = await aiInvoke('ai_bind_ssh_approval', { conversationId, target, mode, draft });
        if (current()) {
          if (this.current) this.current.sshApproval = approval;
          else this.draftSshApproval = approval;
        }
        return approval;
      });
      local.sshApprovalChange = operation;
      return operation;
    },
    async loadList() {
      const local = session(this), revision = ++local.list;
      const history = await aiInvoke('ai_list_conversations');
      if (revision !== local.list) return;
      this.workspaces = history.workspaces;
      this.conversations = history.conversations;
      if (!this.workspaces.some(workspace => workspace.id === this.workspaceId)) {
        this.workspaceId = this.workspaces[0]?.id ?? null;
      }
    },
    async open(id) {
      const local = session(this), revision = ++local.view;
      const conversation = await aiInvoke('ai_get_conversation', { id });
      if (revision !== local.view) return false;
      const entry = this.conversations.find(item => item.id === id);
      const workspaceId = entry?.workspaceId ?? (this.active?.conversationId === id ? this.active.workspaceId : null);
      if (workspaceId) this.workspaceId = workspaceId;
      if (this.active) this.active.pendingInComposer = false;
      this.current = this.active?.conversationId === id && this.active.snapshot
        ? { ...this.active.snapshot, draft: conversation.draft } : conversation;
      this.selectedModelId = conversation.modelId || this.settings.defaultModelId;
      if (!this.selectedModel) this.selectedModelId = this.settings.defaultModelId;
      this.error = '';
      return true;
    },
    newConversation(workspaceId = this.workspaceId) {
      if (!this.workspaces.some(workspace => workspace.id === workspaceId)) return false;
      session(this).view++;
      if (this.active) this.active.pendingInComposer = false;
      this.workspaceId = workspaceId;
      this.current = null;
      this.emptyDraft = '';
      this.draftSshApproval = null;
      this.emptyConversationRevision++;
      this.selectedModelId = this.settings.defaultModelId;
      this.error = '';
      return true;
    },
    async createWorkspace(name) {
      const workspace = await aiInvoke('ai_create_workspace', { name });
      await this.loadList();
      return workspace;
    },
    async renameWorkspace(id, name) {
      await aiInvoke('ai_rename_workspace', { id, name });
      await this.loadList();
    },
    async putWorkspaceFirst(id) {
      await aiInvoke('ai_put_workspace_first', { id });
      await this.loadList();
    },
    async moveConversation(id, workspaceId) {
      await aiInvoke('ai_move_conversation', { id, workspaceId });
      if (this.current?.id === id) this.workspaceId = workspaceId;
      if (this.active?.conversationId === id) this.active.workspaceId = workspaceId;
      await this.loadList();
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
      if (this.current?.id === id) this.newConversation();
      await this.loadList();
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
    async verifyTaskResults(messageId, steps) {
      const conversationId = this.current?.id, view = session(this).view;
      if (!conversationId || this.active) return;
      this.error = '';
      const conversation = await aiInvoke('ai_verify_task_results', { conversationId, messageId, steps });
      if (session(this).view === view && this.current?.id === conversationId) {
        this.current = { ...conversation, draft: this.current.draft };
      }
    },
    async waitForIdle() { await session(this).run; },
    async send(content, regenerateId = null, readGrant = null, execGrant = null, taskIntent = {}) {
      if (this.active || this.importingText || !this.selectedModel) return false;
      const model = this.selectedModel;
      const recipient = modelRecipient(model.service);
      const local = session(this), view = local.view;
      const requestId = crypto.randomUUID();
      const creating = !this.current;
      const workspaceId = this.workspaceId;
      if (creating && !workspaceId) return false;
      const conversationId = this.current?.id ?? crypto.randomUUID();
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
      this.active = { requestId, conversationId, workspaceId, pendingInComposer: creating,
        seq: -1, stage: 'preparing', snapshot: null, cancelRequested: false };
      const updateCurrent = conversation => {
        if (this.current?.id === conversationId) {
          const draft = this.current.draft;
          this.current = conversation;
          this.current.draft = draft;
        } else if (creating && local.view === view && !this.current && this.active?.pendingInComposer) {
          this.current = conversation;
          this.current.draft = this.emptyDraft;
          this.emptyDraft = '';
          this.active.pendingInComposer = false;
        }
      };
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
        if (event.kind === 'terminal_notice') {
          const message = this.active.snapshot?.messages.find(message => message.id === event.messageId);
          if (message && event.text) (message.terminalNotices ||= []).push(event.text);
          return;
        }
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
          updateCurrent(event.conversation);
        }
        // Cancellation while the command was being dispatched must reach Rust too.
        if (event.kind === 'started' && this.active.cancelRequested) void this.stop().catch(e => { this.error = String(e); });
      };
      const run = (async () => {
        try {
          if (this.active.cancelRequested) return;
          const conversation = await aiInvoke('ai_send', { request: { conversationId, requestId, workspaceId: creating ? workspaceId : null,
            modelId: model.id, content, regenerateId, confirmedServiceId: model.service.id, recipient,
            confirmHistoryRecipient: !!taskIntent.confirmHistoryRecipient, readGrant, execGrant,
            diagnosisTarget: taskIntent.diagnosisTarget || null, analyzeMessageId: taskIntent.analyzeMessageId || null,
            terminalTarget: taskIntent.terminalTarget || null,
            draftSshApproval: creating ? this.draftSshApproval : null,
            continueDiagnosis: taskIntent.continueDiagnosis || null, cmdDirectory: taskIntent.cmdDirectory || null,
            deploymentTarget: taskIntent.deploymentTarget || null, skill: taskIntent.skill || null }, channel });
          flush();
          accepted = true;
          updateCurrent(conversation);
        } catch (error) {
          flush();
          const message = String(error);
          const partial = this.active?.snapshot?.messages.at(-1);
          const inline = partial?.status === 'streaming' && this.current?.id === conversationId;
          if (!inline && (local.view === view || this.current?.id === conversationId)) this.error = message;
          if (partial?.status === 'streaming') {
            partial.status = 'failed'; partial.error = message;
            if (partial.task) partial.task.status = 'failed';
            for (const tool of partial.tools || []) if (['running', 'waiting_approval', 'approved'].includes(tool.status)) {
              tool.approvalId = null; tool.approvalExpiresAt = null;
              const uncertain = tool.execution && ['dispatching', 'running'].includes(tool.execution.outcome);
              const local = tool.name === 'execute_local_cmd' || tool.arguments?.skill?.targetKind === 'cmd';
              const edit = tool.name === 'apply_remote_edit';
              tool.status = uncertain ? 'unknown' : 'interrupted';
              if (tool.execution) { tool.execution.outcome = uncertain ? 'unknown' : 'not_started'; tool.execution.outputComplete = false; }
              tool.result = uncertain
                ? (edit ? '聊天连接中断，请核实远程文件的实际内容' : local ? '聊天连接中断，本地命令结果未知，请先核实状态' : '聊天连接中断，无法确认远端进程状态，请先核实')
                : '聊天连接中断，未派发操作或未获取查询结果';
            }
          }
        } finally {
          if (readGrant) await aiInvoke('ai_revoke_read_scope', { token: readGrant }).catch(() => {});
          if (execGrant) await aiInvoke('ai_revoke_exec_scope', { token: execGrant }).catch(() => {});
          flush();
          if (!accepted && !regenerateId && !taskIntent.analyzeMessageId) {
            if (this.current?.id === conversationId) this.current.draft ||= content;
            else if (creating && local.view === view && !this.current) this.emptyDraft ||= content;
          }
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
