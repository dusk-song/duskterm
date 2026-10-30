<script setup>
import '@/components/ui/dialog/management-dialog.css';
import { Plus, Search, Users } from '@lucide/vue';
import Button from '@/components/ui/button/Button.vue';
import Checkbox from '@/components/ui/checkbox/Checkbox.vue';
import Dialog from '@/components/ui/dialog/Dialog.vue';
import DialogContent from '@/components/ui/dialog/DialogContent.vue';
import DialogDescription from '@/components/ui/dialog/DialogDescription.vue';
import DialogFooter from '@/components/ui/dialog/DialogFooter.vue';
import DialogHeader from '@/components/ui/dialog/DialogHeader.vue';
import DialogTitle from '@/components/ui/dialog/DialogTitle.vue';
import Input from '@/components/ui/input/Input.vue';
import Select from '@/components/ui/select/Select.vue';
import SelectContent from '@/components/ui/select/SelectContent.vue';
import SelectItem from '@/components/ui/select/SelectItem.vue';
import SelectTrigger from '@/components/ui/select/SelectTrigger.vue';
import SelectValue from '@/components/ui/select/SelectValue.vue';
import { confirm } from '@/composables/useConfirm';
import { toast } from '@/composables/useToast';
import { computed, ref, watch } from 'vue';
import { useSshStore } from '@/stores/ssh';
import {
  cloneSyncChannels,
  createSyncChannel,
  createSyncChannelName,
  reassignSessionsToChannel,
  reconcileSyncChannels,
} from '@/utils/syncInputChannels';

const props = defineProps({
  visible: Boolean,
  activeKey: String,
  syncChannels: {
    type: Array,
    default: () => [],
  },
  selectedChannelId: {
    type: String,
    default: '',
  },
  replaceSyncChannels: {
    type: Function,
    required: true,
  },
  clearSyncChannels: {
    type: Function,
    required: true,
  },
  setSelectedSyncChannelId: {
    type: Function,
    required: true,
  },
});

const emit = defineEmits(['update:visible', 'sync-changed']);

const sshStore = useSshStore();

const dialogOpen = computed({
  get: () => props.visible,
  set: (value) => {
    if (!value) closeDialog();
  },
});

const syncChannelsDraft = ref([]);
const selectedChannelIdDraft = ref('');
const sessionSearchText = ref('');

const openSessions = computed(() => Array.isArray(sshStore.sessions) ? sshStore.sessions : []);
const openSessionIds = computed(() => openSessions.value.map((session) => session.id));
const connectedSessionIds = computed(() =>
  openSessions.value
    .filter((session) => session.status === 'connected')
    .map((session) => session.id),
);

const channelPreviewState = computed(() => reconcileSyncChannels(syncChannelsDraft.value, {
  sessionIds: openSessionIds.value,
  connectedSessionIds: connectedSessionIds.value,
}));

const previewChannels = computed(() => channelPreviewState.value.channels || []);

const currentChannelDraft = computed(() =>
  syncChannelsDraft.value.find((channel) => channel.id === selectedChannelIdDraft.value) || null,
);

const currentChannelPreview = computed(() =>
  previewChannels.value.find((channel) => channel.id === selectedChannelIdDraft.value) || null,
);

const sessionCandidates = computed(() => {
  const keyword = sessionSearchText.value.trim().toLowerCase();
  const sessions = [...openSessions.value].sort((left, right) => {
    if (left.status === right.status) {
      return (left.name || left.host || '').localeCompare(right.name || right.host || '');
    }
    return left.status === 'connected' ? -1 : 1;
  });

  if (!keyword) return sessions;

  return sessions.filter((session) => {
    const fields = [
      session.name,
      session.host,
      session.username,
      session.group,
      session.status,
    ].map((field) => String(field || '').toLowerCase());
    return fields.some((field) => field.includes(keyword));
  });
});

const groupedSessions = computed(() => {
  const groups = new Map();
  sessionCandidates.value.forEach((session) => {
    const groupKey = session.group || '未分组';
    if (!groups.has(groupKey)) groups.set(groupKey, []);
    groups.get(groupKey).push(session);
  });

  return Array.from(groups.entries()).sort((left, right) => {
    if (left[0] === '未分组') return 1;
    if (right[0] === '未分组') return -1;
    return left[0].localeCompare(right[0]);
  });
});

const currentChannelMemberIds = computed(() => currentChannelDraft.value?.sessionIds || []);
const currentChannelPrimaryCandidates = computed(() => {
  const selected = new Set(currentChannelMemberIds.value);
  return openSessions.value.filter((session) => selected.has(session.id));
});

const currentChannelMemberCount = computed(() => currentChannelPreview.value?.connectedCount || 0);
const currentChannelBroadcastEnabled = computed(() => !!currentChannelPreview.value?.broadcastEnabled);

const getNextChannelName = () => {
  const existing = new Set(syncChannelsDraft.value.map((channel) => channel.name));
  let index = syncChannelsDraft.value.length + 1;
  let nextName = createSyncChannelName(index);
  while (existing.has(nextName)) {
    index += 1;
    nextName = createSyncChannelName(index);
  }
  return nextName;
};

const getAssignedChannel = (sessionId) =>
  syncChannelsDraft.value.find((channel) => channel.sessionIds.includes(sessionId)) || null;

const isSessionSelectedInCurrentChannel = (sessionId) =>
  currentChannelMemberIds.value.includes(sessionId);

const syncPreviewLabel = computed(() => {
  if (!currentChannelPreview.value) return '未选择频道';
  const roleLabel = currentChannelPreview.value.sourceMode === 'primary'
    ? (String(currentChannelPreview.value.primarySessionId || '') === String(props.activeKey || '') ? '主控' : '跟随')
    : '组内任意';
  const sendLabel = currentChannelPreview.value.sendMode === 'line' ? '回车' : '实时';
  return `${currentChannelPreview.value.name} | 同步 ${currentChannelPreview.value.connectedCount} | ${roleLabel} | ${sendLabel}`;
});

const cloneFromProps = () => {
  syncChannelsDraft.value = cloneSyncChannels(props.syncChannels);
  selectedChannelIdDraft.value = syncChannelsDraft.value.some((channel) => channel.id === props.selectedChannelId)
    ? props.selectedChannelId
    : (syncChannelsDraft.value[0]?.id || '');
  sessionSearchText.value = '';
};

const closeDialog = () => {
  emit('update:visible', false);
};

const createNewChannel = () => {
  const nextChannel = createSyncChannel({
    name: getNextChannelName(),
    enabled: true,
  });
  syncChannelsDraft.value = [...syncChannelsDraft.value, nextChannel];
  selectedChannelIdDraft.value = nextChannel.id;
};

const deleteCurrentChannel = () => {
  if (!currentChannelDraft.value) return;
  syncChannelsDraft.value = syncChannelsDraft.value.filter((channel) => channel.id !== currentChannelDraft.value.id);
  selectedChannelIdDraft.value = syncChannelsDraft.value[0]?.id || '';
};

const updateCurrentChannel = (patch) => {
  if (!currentChannelDraft.value) return;
  syncChannelsDraft.value = syncChannelsDraft.value.map((channel) =>
    channel.id === currentChannelDraft.value.id
      ? { ...channel, ...patch }
      : channel,
  );
};

const setCurrentChannelSessionIds = (sessionIds) => {
  if (!currentChannelDraft.value) return;
  syncChannelsDraft.value = reassignSessionsToChannel(syncChannelsDraft.value, currentChannelDraft.value.id, sessionIds);
};

const toggleSessionMembership = (sessionId) => {
  if (!currentChannelDraft.value) {
    createNewChannel();
  }

  const currentIds = new Set(currentChannelMemberIds.value);
  if (currentIds.has(sessionId)) {
    currentIds.delete(sessionId);
  } else {
    currentIds.add(sessionId);
  }
  setCurrentChannelSessionIds([...currentIds]);
};

const selectAllSessions = () => {
  setCurrentChannelSessionIds(sessionCandidates.value.map((session) => session.id));
};

const selectOnlyActiveSession = () => {
  if (!props.activeKey) {
    toast.info('当前没有激活会话');
    return;
  }
  const exists = openSessions.value.some((session) => session.id === props.activeKey);
  if (!exists) {
    toast.warning('当前激活会话已不存在');
    return;
  }
  setCurrentChannelSessionIds([props.activeKey]);
};

const selectSameGroupAsActive = () => {
  if (!props.activeKey) {
    toast.info('当前没有激活会话');
    return;
  }

  const activeSession = openSessions.value.find((session) => session.id === props.activeKey);
  if (!activeSession) {
    toast.warning('当前激活会话已不存在');
    return;
  }

  const groupName = activeSession.group || '';
  const matched = openSessions.value.filter((session) => (session.group || '') === groupName);
  if (matched.length === 0) {
    toast.info('当前会话没有可复用的同组成员');
    return;
  }

  setCurrentChannelSessionIds(matched.map((session) => session.id));
};

const clearCurrentChannelSessions = () => {
  setCurrentChannelSessionIds([]);
};

const applySyncChannels = () => {
  const trimmedChannels = syncChannelsDraft.value
    .map((channel) => ({
      ...channel,
      name: String(channel.name || '').trim() || getNextChannelName(),
      sessionIds: [...(channel.sessionIds || [])],
    }))
    .filter((channel) => channel.sessionIds.length > 0);

  props.replaceSyncChannels(trimmedChannels, selectedChannelIdDraft.value);
  props.setSelectedSyncChannelId(selectedChannelIdDraft.value);
  emit('sync-changed');
  closeDialog();
  toast.success('同步输入频道已更新');
};

const clearAllChannels = () => {
  if (!syncChannelsDraft.value.length) {
    closeDialog();
    return;
  }

  confirm({
    title: '清空同步频道',
    content: '这会关闭当前所有临时同步频道，并释放所有会话成员关系。是否继续？',
    okText: '确认清空',
    cancelText: '取消',
    okButtonProps: { danger: true },
    onOk: () => {
      props.clearSyncChannels();
      syncChannelsDraft.value = [];
      selectedChannelIdDraft.value = '';
      emit('sync-changed');
      closeDialog();
      toast.success('已关闭所有同步频道');
    },
  });
};

watch(() => props.visible, (visible) => {
  if (visible) {
    cloneFromProps();
  }
});

watch(selectedChannelIdDraft, (channelId) => {
  props.setSelectedSyncChannelId(channelId);
});

watch(currentChannelMemberIds, (nextIds) => {
  if (!currentChannelDraft.value) return;
  if (currentChannelDraft.value.sourceMode !== 'primary') return;
  if (nextIds.includes(currentChannelDraft.value.primarySessionId)) return;
  updateCurrentChannel({
    primarySessionId: nextIds[0] || '',
  });
});

watch(() => currentChannelDraft.value?.sourceMode, (mode) => {
  if (!currentChannelDraft.value) return;
  if (mode === 'primary' && !currentChannelDraft.value.primarySessionId) {
    updateCurrentChannel({
      primarySessionId: currentChannelMemberIds.value[0] || '',
    });
  }
});
</script>

<template>
  <Dialog v-model:open="dialogOpen" modal>
    <DialogContent showCloseButton draggable class="management-dialog sync-input-dialog">
      <DialogHeader>
        <DialogTitle>同步输入频道</DialogTitle>
        <DialogDescription class="sr-only">管理临时同步输入频道、会话成员与广播规则</DialogDescription>
      </DialogHeader>

      <div class="sync-dialog-layout">
        <aside class="sync-channel-sidebar" aria-label="频道列表">
          <div class="management-section-head">
            <h3 class="management-section-title">频道列表</h3>
            <Button variant="ghost" @click="createNewChannel"><Plus />新建</Button>
          </div>
          <div class="sync-channel-list">
            <button v-for="channel in previewChannels" :key="channel.id" type="button"
              class="sync-channel-item" :class="{ 'is-selected': channel.id === selectedChannelIdDraft }"
              :aria-pressed="channel.id === selectedChannelIdDraft"
              @click="selectedChannelIdDraft = channel.id">
              <Users :size="16" class="sync-channel-icon" />
              <span class="sync-channel-copy">
                <span class="sync-channel-name">{{ channel.name }}</span>
                <span class="management-caption">{{ channel.connectedCount }} 个在线成员 · {{ channel.broadcastEnabled ? '同步中' : '暂停中' }}</span>
              </span>
            </button>
            <div v-if="previewChannels.length === 0" class="management-empty">还没有同步频道，先新建一个</div>
          </div>
        </aside>

        <div class="sync-dialog-main">
          <section class="sync-session-pane" aria-label="会话选择">
            <div class="management-section-head">
              <h3 class="management-section-title">会话选择</h3>
              <span class="management-caption">已选 {{ currentChannelMemberIds.length }} 项</span>
            </div>
            <div class="sync-search">
              <Search :size="16" aria-hidden="true" />
              <Input v-model="sessionSearchText" placeholder="搜索会话、主机、分组" aria-label="搜索会话、主机、分组" />
            </div>
            <div class="sync-batch-actions">
              <Button variant="ghost" @click="selectAllSessions" :disabled="!currentChannelDraft">全选</Button>
              <Button variant="ghost" @click="selectOnlyActiveSession" :disabled="!currentChannelDraft">当前</Button>
              <Button variant="ghost" @click="selectSameGroupAsActive" :disabled="!currentChannelDraft">同组</Button>
              <Button variant="ghost" @click="clearCurrentChannelSessions" :disabled="!currentChannelDraft">清空</Button>
            </div>
            <div class="sync-session-list">
              <div v-if="groupedSessions.length === 0" class="management-empty">暂无可管理的会话</div>
              <div v-for="[groupName, sessions] in groupedSessions" :key="groupName" class="sync-session-group">
                <div class="management-caption sync-group-label">{{ groupName }}</div>
                <div class="sync-session-rows">
                  <button v-for="session in sessions" :key="session.id" type="button" role="checkbox"
                    :aria-checked="isSessionSelectedInCurrentChannel(session.id)"
                    class="sync-session-item" :class="{ 'is-selected': isSessionSelectedInCurrentChannel(session.id) }"
                    @click="toggleSessionMembership(session.id)">
                    <Checkbox as="span" :model-value="isSessionSelectedInCurrentChannel(session.id)"
                      :tabindex="-1" aria-hidden="true" class="pointer-events-none" />
                    <span class="sync-session-copy">
                      <span class="sync-session-name">{{ session.name || ((session.username || 'user') + '@' + (session.host || 'host')) }}</span>
                      <span class="management-caption sync-session-host">{{ session.host || '无主机信息' }}</span>
                    </span>
                    <span class="sync-session-meta">
                      <span class="sync-session-status" :class="{ 'is-online': session.status === 'connected' }">
                        <span class="sync-status-dot" />{{ session.status === 'connected' ? '在线' : '离线' }}
                      </span>
                      <span v-if="getAssignedChannel(session.id)?.id && !isSessionSelectedInCurrentChannel(session.id)"
                        class="management-caption sync-assigned-channel">已在 {{ getAssignedChannel(session.id)?.name }}</span>
                    </span>
                  </button>
                </div>
              </div>
            </div>
          </section>

          <section class="sync-rules" aria-label="频道规则">
            <div class="management-section-head">
              <h3 class="management-section-title">频道规则</h3>
              <Button variant="ghost" class="management-danger-action" @click="deleteCurrentChannel" :disabled="!currentChannelDraft">删除频道</Button>
            </div>
            <div v-if="!currentChannelDraft" class="management-empty sync-rules-empty">先新建或选择一个频道，再配置会话与规则</div>
            <div v-else class="sync-rules-content">
              <div class="management-field">
                <label for="sync-channel-name" class="management-label">频道名称</label>
                <Input id="sync-channel-name" :model-value="currentChannelDraft.name"
                  @update:model-value="(value) => updateCurrentChannel({ name: value })" />
              </div>
              <div class="management-field">
                <div id="sync-source-label" class="management-label">输入来源</div>
                <div class="management-segmented" role="group" aria-labelledby="sync-source-label">
                  <button type="button" :aria-pressed="currentChannelDraft.sourceMode === 'all'"
                    @click="updateCurrentChannel({ sourceMode: 'all', primarySessionId: '' })">组内任意</button>
                  <button type="button" :aria-pressed="currentChannelDraft.sourceMode === 'primary'"
                    @click="updateCurrentChannel({ sourceMode: 'primary', primarySessionId: currentChannelDraft.primarySessionId || currentChannelMemberIds[0] || '' })">仅主控</button>
                </div>
              </div>
              <div v-if="currentChannelDraft.sourceMode === 'primary'" class="management-field">
                <label for="sync-primary-session" class="management-label">主控会话</label>
                <Select :model-value="currentChannelDraft.primarySessionId"
                  @update:model-value="(value) => updateCurrentChannel({ primarySessionId: value })">
                  <SelectTrigger id="sync-primary-session" class="w-full"><SelectValue placeholder="选择主控会话" /></SelectTrigger>
                  <SelectContent class="management-dialog-options" position="popper" side="bottom" align="start" :side-offset="4" :collision-padding="16">
                    <SelectItem v-for="session in currentChannelPrimaryCandidates" :key="session.id" :value="session.id">
                      {{ session.name || ((session.username || 'user') + '@' + (session.host || 'host')) }}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <div class="management-field">
                <div id="sync-send-label" class="management-label">发送方式</div>
                <div class="management-segmented" role="group" aria-labelledby="sync-send-label">
                  <button type="button" :aria-pressed="currentChannelDraft.sendMode === 'realtime'"
                    @click="updateCurrentChannel({ sendMode: 'realtime' })">实时</button>
                  <button type="button" :aria-pressed="currentChannelDraft.sendMode === 'line'"
                    @click="updateCurrentChannel({ sendMode: 'line' })">回车</button>
                </div>
              </div>
              <div class="sync-preview">
                <h4 class="management-section-title">标题栏预览</h4>
                <div class="sync-preview-label">{{ syncPreviewLabel }}</div>
                <div class="sync-preview-status">
                  <span class="management-caption">在线成员 <strong>{{ currentChannelMemberCount }}</strong></span>
                  <span class="sync-session-status" :class="{ 'is-online': currentChannelBroadcastEnabled }">
                    <span class="sync-status-dot" />{{ currentChannelBroadcastEnabled ? '同步中' : '暂停中' }}
                  </span>
                </div>
              </div>
            </div>
          </section>
        </div>
      </div>

      <DialogFooter class="management-footer">
        <Button variant="ghost" class="management-danger-action sync-clear-all" @click="clearAllChannels" :disabled="syncChannelsDraft.length === 0">清空频道</Button>
        <Button variant="ghost" @click="closeDialog">取消</Button>
        <Button @click="applySyncChannels">应用频道</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>

<style scoped>
.sync-dialog-layout { display: flex; min-height: 0; flex: 1; gap: 16px; overflow: hidden; }
.sync-channel-sidebar { display: flex; min-width: 0; flex: 0 0 210px; flex-direction: column; gap: 8px; padding-right: 16px; border-right: 1px solid var(--app-border-shadow); }
.sync-channel-list { min-height: 0; overflow-y: auto; }
.sync-channel-item { display: flex; width: 100%; align-items: flex-start; gap: 8px; margin-bottom: 4px; padding: 10px; border: 0; border-radius: 8px; color: var(--app-text-secondary); background: transparent; text-align: left; transition: background var(--app-motion-control); }
.sync-channel-item:hover, .sync-session-item:hover { background: hsl(var(--foreground) / 0.05); }
.sync-channel-item.is-selected, .sync-session-item.is-selected { color: var(--app-text); background: hsl(var(--primary) / 0.12); }
.sync-channel-item.is-selected { box-shadow: inset 2px 0 hsl(var(--primary)); }
.sync-channel-item:focus-visible, .sync-session-item:focus-visible { outline: none; box-shadow: var(--app-focus-shadow); }
.sync-channel-item:active, .sync-session-item:active { background: hsl(var(--primary) / 0.18); }
.sync-channel-icon { flex-shrink: 0; margin-top: 2px; }
.sync-channel-copy, .sync-session-copy { display: flex; min-width: 0; flex: 1; flex-direction: column; gap: 2px; }
.sync-channel-name { overflow: hidden; font-size: var(--app-font-body-size); font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
.sync-channel-copy .management-caption { overflow-wrap: anywhere; }
.sync-channel-list, .sync-session-list, .sync-rules-content { scrollbar-gutter: stable; }
.sync-dialog-main { display: grid; min-width: 0; flex: 1; grid-template-columns: minmax(0, 1fr) 280px; gap: 16px; overflow: hidden; }
.sync-session-pane { display: flex; min-width: 0; min-height: 0; flex-direction: column; gap: 8px; }
.sync-search { position: relative; }
.sync-search > svg { position: absolute; z-index: 1; top: 7px; left: 10px; color: var(--app-text-muted); pointer-events: none; }
.sync-search :deep(input) { padding-left: 32px; }
.sync-batch-actions { display: flex; flex-shrink: 0; flex-wrap: wrap; gap: 4px; }
.sync-session-list { min-height: 0; flex: 1; overflow-y: auto; }
.sync-session-group { margin-bottom: 16px; }
.sync-group-label { margin-bottom: 8px; font-weight: 600; overflow-wrap: anywhere; }
.sync-session-rows { display: flex; flex-direction: column; gap: 4px; }
.sync-session-item { display: flex; width: 100%; min-height: 48px; align-items: center; gap: 8px; padding: 8px; border: 0; border-radius: 8px; background: transparent; text-align: left; transition: background var(--app-motion-control); }
.sync-session-name, .sync-session-host { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sync-session-name { color: var(--app-text); font-size: var(--app-font-body-size); font-weight: 500; }
.sync-session-meta { display: flex; max-width: 100px; flex-shrink: 0; flex-direction: column; align-items: flex-end; gap: 4px; }
.sync-session-status { display: inline-flex; flex-shrink: 0; align-items: center; gap: 7px; color: var(--app-text-secondary); font-size: var(--app-font-caption-size); white-space: nowrap; }
.sync-session-status.is-online { color: var(--app-toast-success); }
.sync-status-dot { width: 7px; height: 7px; flex-shrink: 0; border-radius: 50%; background: currentColor; }
.sync-assigned-channel { width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sync-rules { display: flex; min-width: 0; min-height: 0; flex-direction: column; gap: 8px; padding-left: 16px; border-left: 1px solid var(--app-border-shadow); }
.sync-rules-content { display: flex; min-height: 0; flex-direction: column; gap: 16px; overflow-y: auto; padding-right: 4px; }
.sync-rules-empty { margin: auto 0; text-align: center; }
.sync-preview { display: flex; flex-direction: column; gap: 10px; padding-top: 12px; border-top: 1px solid var(--app-border-shadow); }
.sync-preview-label { overflow-wrap: anywhere; }
.sync-preview-status { display: flex; flex-wrap: wrap; justify-content: space-between; gap: 12px; }
.sync-preview-status strong { margin-left: 8px; color: var(--app-text); font-weight: 500; }
.sync-clear-all { margin-right: auto; }
@media (max-width: 900px) {
  .sync-dialog-main { grid-template-columns: minmax(0, 1fr); overflow-y: auto; }
  .sync-session-pane { min-height: 260px; flex-shrink: 0; }
  .sync-session-list { max-height: 340px; flex: none; }
  .sync-rules { min-height: auto; flex-shrink: 0; padding: 16px 0 0; border-left: 0; border-top: 1px solid var(--app-border-shadow); }
  .sync-rules-content { overflow: visible; padding-right: 0; }
  .sync-rules-empty { margin: 0; }
}
@media (max-width: 680px) {
  .sync-dialog-layout { display: block; overflow-y: auto; }
  .sync-channel-sidebar { max-height: 190px; gap: 8px; margin-bottom: 16px; padding: 0 0 16px; border-right: 0; border-bottom: 1px solid var(--app-border-shadow); }
  .sync-dialog-main { display: block; overflow: visible; }
  .sync-session-pane { min-height: auto; }
  .sync-session-list { max-height: 340px; }
  .sync-rules { margin-top: 16px; }
}
</style>
