<script setup>
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
    <DialogContent showCloseButton
      class="flex h-[min(620px,calc(100vh-4rem))] max-h-[calc(100vh-4rem)] w-[1020px] max-w-[94vw] flex-col sm:max-w-[94vw]">
      <DialogHeader>
        <DialogTitle>同步输入频道</DialogTitle>
        <DialogDescription class="sr-only">管理临时同步输入频道、会话成员与广播规则</DialogDescription>
      </DialogHeader>

      <div class="flex min-h-0 flex-1 gap-3 overflow-hidden px-1 pb-1">
        <div class="sync-channel-sidebar flex w-[220px] shrink-0 flex-col rounded-md border border-border bg-muted/20">
          <div class="flex items-center justify-between border-b border-border px-3 py-2">
            <span class="text-xs font-semibold text-foreground">频道列表</span>
            <Button size="sm" variant="outline" @click="createNewChannel">新建频道</Button>
          </div>
          <div class="flex-1 space-y-1 overflow-y-auto p-2">
            <button v-for="channel in previewChannels" :key="channel.id" type="button"
              class="w-full rounded-md border px-3 py-2 text-left transition-colors"
              :class="channel.id === selectedChannelIdDraft ? 'border-primary bg-primary/10 text-foreground' : 'border-transparent bg-transparent text-muted-foreground hover:border-border hover:bg-accent/30 hover:text-foreground'"
              @click="selectedChannelIdDraft = channel.id">
              <div class="flex items-center justify-between gap-2">
                <span class="truncate text-sm font-medium">{{ channel.name }}</span>
                <span class="rounded-full border border-border px-2 py-0.5 text-[10px]">
                  {{ channel.connectedCount }}
                </span>
              </div>
              <div class="mt-1 text-[11px]" :class="channel.broadcastEnabled ? 'text-emerald-500' : 'text-muted-foreground'">
                {{ channel.broadcastEnabled ? '同步中' : '暂停中' }}
              </div>
            </button>

            <div v-if="previewChannels.length === 0" class="px-2 py-4 text-xs text-muted-foreground">
              还没有同步频道，先新建一个。
            </div>
          </div>
        </div>

        <div class="flex min-w-0 flex-1 gap-3 overflow-hidden">
          <div class="flex min-w-0 flex-[1.2] flex-col rounded-md border border-border bg-muted/10">
            <div class="flex items-center justify-between border-b border-border px-3 py-2">
              <span class="text-xs font-semibold text-foreground">会话选择</span>
              <div class="flex flex-wrap gap-1">
                <Button size="sm" variant="outline" @click="selectAllSessions" :disabled="!currentChannelDraft">全选</Button>
                <Button size="sm" variant="outline" @click="selectOnlyActiveSession" :disabled="!currentChannelDraft">当前</Button>
                <Button size="sm" variant="outline" @click="selectSameGroupAsActive" :disabled="!currentChannelDraft">同组</Button>
                <Button size="sm" variant="outline" @click="clearCurrentChannelSessions" :disabled="!currentChannelDraft">清空</Button>
              </div>
            </div>

            <div class="px-3 pb-2 pt-3">
              <Input v-model="sessionSearchText" size="sm" placeholder="搜索会话、主机、分组..." class="w-full" />
            </div>

            <div class="min-h-0 flex-1 overflow-y-auto px-3 pb-3">
              <div v-if="groupedSessions.length === 0" class="py-6 text-center text-sm text-muted-foreground">
                暂无可管理的会话
              </div>

              <div v-for="[groupName, sessions] in groupedSessions" :key="groupName" class="mb-3">
                <div class="mb-1 text-[11px] font-semibold text-muted-foreground">{{ groupName }}</div>
                <div class="space-y-1">
                  <button v-for="session in sessions" :key="session.id" type="button"
                    class="flex w-full items-center gap-2 rounded-md border px-2 py-2 text-left transition-colors"
                    :class="isSessionSelectedInCurrentChannel(session.id)
                      ? 'border-primary bg-primary/10'
                      : 'border-transparent hover:border-border hover:bg-accent/30'"
                    @click="toggleSessionMembership(session.id)">
                    <Checkbox :model-value="isSessionSelectedInCurrentChannel(session.id)" />
                    <div class="min-w-0 flex-1">
                      <div class="truncate text-sm font-medium text-foreground">
                        {{ session.name || `${session.username || 'user'}@${session.host || 'host'}` }}
                      </div>
                      <div class="truncate text-[11px] text-muted-foreground">
                        {{ session.host || '无主机信息' }}
                      </div>
                    </div>
                    <div class="flex shrink-0 flex-col items-end gap-1">
                      <span class="text-[10px]"
                        :class="session.status === 'connected' ? 'text-emerald-500' : 'text-amber-500'">
                        {{ session.status === 'connected' ? '在线' : '离线' }}
                      </span>
                      <span v-if="getAssignedChannel(session.id)?.id && !isSessionSelectedInCurrentChannel(session.id)"
                        class="max-w-[90px] truncate text-[10px] text-muted-foreground">
                        已在 {{ getAssignedChannel(session.id)?.name }}
                      </span>
                    </div>
                  </button>
                </div>
              </div>
            </div>
          </div>

          <div class="flex w-[280px] shrink-0 flex-col rounded-md border border-border bg-muted/10">
            <div class="flex items-center justify-between border-b border-border px-3 py-2">
              <span class="text-xs font-semibold text-foreground">频道规则</span>
              <Button size="sm" variant="ghost" @click="deleteCurrentChannel" :disabled="!currentChannelDraft">删除频道</Button>
            </div>

            <div v-if="!currentChannelDraft" class="flex flex-1 items-center justify-center px-4 text-center text-sm text-muted-foreground">
              先新建或选择一个频道，再配置会话与规则。
            </div>

            <div v-else class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-3 py-3">
              <div class="space-y-1">
                <label class="text-xs text-muted-foreground">频道名称</label>
                <Input :model-value="currentChannelDraft.name" size="sm" class="w-full"
                  @update:model-value="(value) => updateCurrentChannel({ name: value })" />
              </div>

              <div class="space-y-2">
                <div class="text-xs text-muted-foreground">输入来源</div>
                <div class="flex gap-1">
                  <button type="button"
                    :class="['px-3 py-1 rounded text-xs font-medium transition-colors', currentChannelDraft.sourceMode === 'all' ? 'bg-primary text-primary-foreground' : 'bg-secondary text-secondary-foreground hover:bg-accent']"
                    @click="updateCurrentChannel({ sourceMode: 'all', primarySessionId: '' })">
                    组内任意
                  </button>
                  <button type="button"
                    :class="['px-3 py-1 rounded text-xs font-medium transition-colors', currentChannelDraft.sourceMode === 'primary' ? 'bg-primary text-primary-foreground' : 'bg-secondary text-secondary-foreground hover:bg-accent']"
                    @click="updateCurrentChannel({ sourceMode: 'primary', primarySessionId: currentChannelDraft.primarySessionId || currentChannelMemberIds[0] || '' })">
                    仅主控
                  </button>
                </div>
              </div>

              <div v-if="currentChannelDraft.sourceMode === 'primary'" class="space-y-1">
                <label class="text-xs text-muted-foreground">主控会话</label>
                <Select :model-value="currentChannelDraft.primarySessionId"
                  @update:model-value="(value) => updateCurrentChannel({ primarySessionId: value })">
                  <SelectTrigger size="sm" class="w-full">
                    <SelectValue placeholder="选择主控会话" />
                  </SelectTrigger>
                  <SelectContent position="popper" side="bottom" align="start" :side-offset="4" :collision-padding="16">
                    <SelectItem v-for="session in currentChannelPrimaryCandidates" :key="session.id" :value="session.id">
                      {{ session.name || `${session.username || 'user'}@${session.host || 'host'}` }}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>

              <div class="space-y-2">
                <div class="text-xs text-muted-foreground">发送方式</div>
                <div class="flex gap-1">
                  <button type="button"
                    :class="['px-3 py-1 rounded text-xs font-medium transition-colors', currentChannelDraft.sendMode === 'realtime' ? 'bg-primary text-primary-foreground' : 'bg-secondary text-secondary-foreground hover:bg-accent']"
                    @click="updateCurrentChannel({ sendMode: 'realtime' })">
                    实时
                  </button>
                  <button type="button"
                    :class="['px-3 py-1 rounded text-xs font-medium transition-colors', currentChannelDraft.sendMode === 'line' ? 'bg-primary text-primary-foreground' : 'bg-secondary text-secondary-foreground hover:bg-accent']"
                    @click="updateCurrentChannel({ sendMode: 'line' })">
                    回车
                  </button>
                </div>
              </div>

              <div class="space-y-2 rounded-md border border-border bg-background/70 p-3">
                <div class="text-xs font-semibold text-foreground">标题栏预览</div>
                <div class="rounded-md border border-border bg-muted/20 px-3 py-2 text-xs text-foreground">
                  {{ syncPreviewLabel }}
                </div>
                <div class="grid grid-cols-2 gap-2 text-[11px] text-muted-foreground">
                  <div class="rounded-md border border-border px-2 py-2">在线成员<br><span class="text-foreground">{{ currentChannelMemberCount }}</span></div>
                  <div class="rounded-md border border-border px-2 py-2">广播状态<br><span
                      :class="currentChannelBroadcastEnabled ? 'text-emerald-500' : 'text-amber-500'">{{
                        currentChannelBroadcastEnabled ? '同步中' : '暂停中'
                      }}</span></div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <DialogFooter>
        <Button variant="ghost" @click="closeDialog">取消</Button>
        <Button variant="destructive" @click="clearAllChannels" :disabled="syncChannelsDraft.length === 0">清空频道</Button>
        <Button @click="applySyncChannels">应用频道</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
