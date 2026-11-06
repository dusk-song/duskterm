<script setup>
import Dialog from '@/components/ui/dialog/Dialog.vue';
import DialogContent from '@/components/ui/dialog/DialogContent.vue';
import DialogDescription from '@/components/ui/dialog/DialogDescription.vue';
import DialogHeader from '@/components/ui/dialog/DialogHeader.vue';
import DialogTitle from '@/components/ui/dialog/DialogTitle.vue';
import { confirm } from '@/composables/useConfirm';
import { toast } from '@/composables/useToast';
import {
  Home,
  Sparkles,
  Keyboard,
  Lock,
  Terminal
} from '@lucide/vue';
import { open } from '@tauri-apps/plugin-dialog';
import { computed, defineAsyncComponent, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { useSecurityStore } from '@/stores/security';
import { useCommandHistoryStore } from '@/stores/commandHistory';
import {
  createDefaultDesktopPetNode,
  defaultDesktopPetNodes,
  getDesktopPetAssetFileName,
  inferDesktopPetAssetType,
  resolveDesktopPetAssetUrl
} from '@/utils/desktopPet';
import { invokeCommand } from '@/utils/ipc';
import { normalizeMouseBindingEvent } from '@/utils/keybindings';
import { loadMainUiSettings, normalizeMainUiSettings, saveMainUiSettings } from '@/utils/mainUi';
import { getPreferenceDefaults, loadPreference, savePreference } from '@/utils/preferences';
import { loadTerminalThemeSettings, saveTerminalThemeSettings } from '@/utils/terminalTheme';

// Lazy-loaded sub-panes — only the visible tab's code is loaded, reducing first-open lag
const SettingsAiPane = defineAsyncComponent(() => import('./SettingsAiPane.vue'));
const aiPane = ref(null);
const aiPaneMounted = ref(false);
const SettingsKeybindingsPane = defineAsyncComponent(() => import('./SettingsKeybindingsPane.vue'));
const SettingsMainUiPane = defineAsyncComponent(() => import('./SettingsMainUiPane.vue'));
const SettingsSecurityPane = defineAsyncComponent(() => import('./SettingsSecurityPane.vue'));
const SettingsTerminalPane = defineAsyncComponent(() => import('./SettingsTerminalPane.vue'));

const props = defineProps({
  visible: Boolean
});

const emit = defineEmits(['update:visible']);

const dialogOpen = computed({
  get: () => props.visible,
  set: (v) => { if (!v) handleCancel(); },
});

const securityStore = useSecurityStore();
const commandHistoryStore = useCommandHistoryStore();
const activeKey = ref('security');
watch(activeKey, (key) => { if (key === 'ai') aiPaneMounted.value = true; });

const settingsTabs = [
  { key: 'security', label: '安全锁', icon: Lock },
  { key: 'main-ui', label: '应用', icon: Home },
  { key: 'terminal', label: '终端', icon: Terminal },
  { key: 'ai', label: '模型服务', icon: Sparkles },
  { key: 'keybindings', label: '快捷键', icon: Keyboard },
];
const terminalThemeSettings = ref(loadTerminalThemeSettings());
const commandHistorySettings = ref(loadPreference('commandHistory'));
const mainUiSettings = ref(loadMainUiSettings());
let persistedBackgroundIds = new Set();
const temporaryBackgroundIds = new Set();
const pendingBackgroundCleanupIds = new Set();
const backgroundImporting = ref(false);
const closing = ref(false);
const preferencesReady = ref(false);
let persistedMainUi = JSON.stringify(mainUiSettings.value);
let persistedTerminal = JSON.stringify(terminalThemeSettings.value);
let persistedHistory = JSON.stringify(commandHistorySettings.value);
let persistedBindings = JSON.stringify(keybindingsSnapshot());
let preferenceEffects = Promise.resolve();
function keybindingsSnapshot() { return { ...getPreferenceDefaults('keybindings'), ...loadPreference('keybindings') }; }

const getBackgroundResourceIds = (settings) => {
  const background = normalizeMainUiSettings(settings).background;
  return new Set(background.recentAssets.map((asset) => asset.resourceId).filter(Boolean));
};

const deleteBackgroundResource = async (resourceId) => {
  if (!resourceId) return true;
  for (let attempt = 0; attempt < 2; attempt += 1) {
    try {
      await invokeCommand('delete_background_image', { resourceId });
      return true;
    } catch {
      if (attempt === 0) await new Promise((resolve) => setTimeout(resolve, 120));
    }
  }
  return false;
};

const waitForBackgroundRelease = async () => {
  await new Promise((resolve) => setTimeout(resolve, 50));
};

const deleteBackgroundResources = async (resourceIds) => {
  const uniqueIds = [...new Set(resourceIds)].filter(Boolean);
  const results = await Promise.all(uniqueIds.map(async (resourceId) => ({
    resourceId,
    deleted: await deleteBackgroundResource(resourceId),
  })));
  return results.filter((result) => !result.deleted).map((result) => result.resourceId);
};

const handleBackgroundImported = (resourceId) => {
  if (resourceId) temporaryBackgroundIds.add(resourceId);
};
const selectedDesktopPetNodeId = ref('');

const dispatchMainUiSettingsPreview = () => {
  if (preferencesReady.value && !backgroundImporting.value) persistMainUi();
};

const defaultKeybindings = getPreferenceDefaults('keybindings');

const keybindingItems = [
  { key: 'splitHorizontal', label: '水平拆分', placeholder: 'Ctrl+Shift+U' },
  { key: 'splitVertical', label: '垂直拆分', placeholder: 'Ctrl+Alt+I' },
  { key: 'closeSession', label: '关闭当前会话', placeholder: 'Ctrl+Shift+W' },
  { key: 'closeSplitTerminal', label: '关闭当前分屏终端', placeholder: 'Ctrl+Alt+W' },
  { key: 'nextSession', label: '下一个会话', placeholder: 'Ctrl+Tab' },
  { key: 'prevSession', label: '上一个会话', placeholder: 'Ctrl+Shift+Tab' },
  { key: 'sessionList', label: '会话列表', placeholder: 'Alt+1' },
  { key: 'sftpPanel', label: '文件管理', placeholder: 'Alt+2' },
  { key: 'commandKnowledge', label: '命令知识库', placeholder: 'Alt+3' },
  { key: 'transferList', label: '传输列表', placeholder: 'Alt+4' },
  { key: 'aiPanel', label: '智能助手', placeholder: 'Alt+5' },
  { key: 'sftpParentDirectory', label: 'SFTP 返回上一级', placeholder: 'Mouse4' },
  { key: 'overview', label: '总览模式', placeholder: 'Ctrl+`' },
  { key: 'copySession', label: '复制当前会话', placeholder: 'Ctrl+P' },
  { key: 'toggleFind', label: '终端搜索', placeholder: 'Ctrl+Shift+F' },
  { key: 'selectTerminalSuggestion', label: '选择终端命令建议', placeholder: 'Alt+ArrowDown' }
];

const keybindings = ref({ ...defaultKeybindings });
const bindingActionKey = ref('');

const keybindingLabelMap = new Map(keybindingItems.map((item) => [item.key, item.label]));

const normalizeBindingCombo = (combo) => String(combo || '')
  .trim()
  .replace(/\s+/g, '')
  .toLowerCase();

const keybindingConflictEntries = computed(() => {
  const comboBuckets = new Map();
  for (const item of keybindingItems) {
    const raw = String(keybindings.value[item.key] || '').trim();
    if (!raw) continue;
    const normalized = normalizeBindingCombo(raw);
    if (!normalized) continue;
    if (!comboBuckets.has(normalized)) {
      comboBuckets.set(normalized, { combo: raw, keys: [] });
    }
    comboBuckets.get(normalized).keys.push(item.key);
  }

  const conflicts = [];
  comboBuckets.forEach(({ combo, keys }) => {
    if (keys.length < 2) return;
    conflicts.push({
      combo,
      keys,
      labels: keys.map((key) => keybindingLabelMap.get(key) || key)
    });
  });

  return conflicts;
});

const keybindingConflictMap = computed(() => {
  const map = {};
  keybindingConflictEntries.value.forEach((entry) => {
    entry.keys.forEach((key) => {
      map[key] = entry;
    });
  });
  return map;
});

// Security Lock State
const lockPassword = ref('');
const lockPasswordConfirm = ref('');
const currentLockPassword = ref('');

// Settings are loaded as a batch; initialization must never trigger saves.
let preferenceOpenRevision = 0;
watch(() => props.visible, async (visible) => {
  const revision = ++preferenceOpenRevision;
  preferencesReady.value = false;
  if (!visible) { bindingActionKey.value = ''; return; }
  await nextTick();
  if (!props.visible || revision !== preferenceOpenRevision) return;
  aiPaneMounted.value = activeKey.value === 'ai';
  lockPassword.value = ''; lockPasswordConfirm.value = ''; currentLockPassword.value = '';
  keybindings.value = keybindingsSnapshot();
  terminalThemeSettings.value = loadTerminalThemeSettings();
  commandHistorySettings.value = loadPreference('commandHistory');
  mainUiSettings.value = loadMainUiSettings();
  persistedMainUi = JSON.stringify(mainUiSettings.value);
  persistedTerminal = JSON.stringify(terminalThemeSettings.value);
  persistedHistory = JSON.stringify(commandHistorySettings.value);
  persistedBindings = JSON.stringify(keybindings.value);
  persistedBackgroundIds = getBackgroundResourceIds(mainUiSettings.value);
  ensureSelectedDesktopPetNode();
  await nextTick();
  if (props.visible && revision === preferenceOpenRevision) preferencesReady.value = true;
}, { immediate: true });

function enqueuePreferenceEffect(action) {
  preferenceEffects = preferenceEffects.then(action).catch(error => toast.error(String(error)));
  return preferenceEffects;
}
async function cleanupUnusedBackgrounds() {
  if (backgroundImporting.value) return;
  await waitForBackgroundRelease();
  // Re-read the latest successful references after the wait: a later setting
  // may have selected the resource again while this job was queued.
  const keep = getBackgroundResourceIds(loadMainUiSettings());
  const candidates = [...pendingBackgroundCleanupIds].filter(id => !keep.has(id) && !temporaryBackgroundIds.has(id));
  const failed = new Set(await deleteBackgroundResources(candidates));
  candidates.forEach(id => { if (!failed.has(id)) pendingBackgroundCleanupIds.delete(id); });
}
function persistMainUi() {
  if (!preferencesReady.value || !props.visible || backgroundImporting.value) return;
  const snapshot = normalizeMainUiSettings(mainUiSettings.value);
  const serialized = JSON.stringify(snapshot);
  if (serialized === persistedMainUi) return;
  const previous = JSON.parse(persistedMainUi);
  try { saveMainUiSettings(snapshot); }
  catch {
    mainUiSettings.value = previous;
    toast.error('应用设置保存失败，已恢复上次保存的值');
    return;
  }
  persistedMainUi = serialized;
  const nextIds = getBackgroundResourceIds(snapshot);
  persistedBackgroundIds.forEach(id => { if (!nextIds.has(id)) pendingBackgroundCleanupIds.add(id); });
  nextIds.forEach(id => temporaryBackgroundIds.delete(id));
  persistedBackgroundIds = nextIds;
  window.dispatchEvent(new CustomEvent('main-ui-settings-changed'));
  enqueuePreferenceEffect(async () => {
    if (snapshot.recentSessions.enabled && (!previous.recentSessions.enabled || previous.recentSessions.limit !== snapshot.recentSessions.limit)) {
      try { await invokeCommand('trim_recent_sessions', { limit: snapshot.recentSessions.limit }); }
      catch { toast.error('会话数量已保存，但历史清理失败，重新调整可重试'); }
    }
    await cleanupUnusedBackgrounds();
  });
}
function persistTerminal() {
  if (!preferencesReady.value || !props.visible) return;
  const serialized = JSON.stringify(terminalThemeSettings.value);
  if (serialized === persistedTerminal) return;
  try { saveTerminalThemeSettings(terminalThemeSettings.value); }
  catch { terminalThemeSettings.value = JSON.parse(persistedTerminal); toast.error('终端设置保存失败，已恢复上次保存的值'); return; }
  persistedTerminal = serialized;
  window.dispatchEvent(new CustomEvent('terminal-theme-changed'));
}
function persistCommandHistory() {
  if (!preferencesReady.value || !props.visible) return;
  const serialized = JSON.stringify(commandHistorySettings.value);
  if (serialized === persistedHistory) return;
  try { savePreference('commandHistory', commandHistorySettings.value); }
  catch { commandHistorySettings.value = JSON.parse(persistedHistory); toast.error('命令历史设置保存失败，已恢复上次保存的值'); return; }
  persistedHistory = serialized;
  commandHistoryStore.setEnabled(commandHistorySettings.value.enabled);
}
function commitBinding(action, combo) {
  keybindings.value[action] = combo;
  if (keybindingConflictEntries.value.length) {
    const conflict = keybindingConflictEntries.value.map(e => `${e.labels.join('、')}（${e.combo}）`).join('；');
    keybindings.value = JSON.parse(persistedBindings);
    toast.error(`快捷键存在冲突：${conflict}`);
    return;
  }
  try { savePreference('keybindings', keybindings.value); }
  catch { keybindings.value = JSON.parse(persistedBindings); toast.error('快捷键保存失败'); return; }
  persistedBindings = JSON.stringify(keybindings.value);
  window.dispatchEvent(new CustomEvent('keybindings-changed'));
}
watch(mainUiSettings, persistMainUi, { deep: true, flush: 'post' });
watch(terminalThemeSettings, persistTerminal, { deep: true, flush: 'post' });
watch(commandHistorySettings, persistCommandHistory, { deep: true, flush: 'post' });
watch(backgroundImporting, importing => { if (!importing) { persistMainUi(); enqueuePreferenceEffect(cleanupUnusedBackgrounds); } });

const handleCancel = async () => {
  if (closing.value || backgroundImporting.value || aiPane.value?.isBusy()) return;
  await nextTick();
  if (aiPane.value?.isDirty()) {
    try { await confirm({ title: '放弃未保存的服务配置？', content: '当前 AI 服务未保存的修改将丢弃。' }); } catch { return; }
  }
  closing.value = true;
  try {
    persistMainUi(); persistTerminal(); persistCommandHistory();
    aiPaneMounted.value = false;
    temporaryBackgroundIds.forEach(id => pendingBackgroundCleanupIds.add(id));
    temporaryBackgroundIds.clear();
    await enqueuePreferenceEffect(cleanupUnusedBackgrounds);
    if (pendingBackgroundCleanupIds.size) toast.error('部分旧背景文件暂时无法清理，将在下次启动时重试');
    emit('update:visible', false);
  } finally { closing.value = false; }
};

onBeforeUnmount(() => {
  preferencesReady.value = false;
  temporaryBackgroundIds.forEach(id => pendingBackgroundCleanupIds.add(id));
  temporaryBackgroundIds.clear();
  void enqueuePreferenceEffect(cleanupUnusedBackgrounds);
});

const previewTerminalTheme = () => { persistTerminal(); };

const clearCommandHistory = () => {
  confirm({
    title: '清空命令历史',
    content: '确认清空全部命令历史？',
    okText: '清空',
    cancelText: '取消',
    okButtonProps: { danger: true },
    onOk: async () => {
      try {
        await commandHistoryStore.clear();
        toast.success('命令历史已清空');
      } catch (error) {
        toast.error(`清空命令历史失败：${error}`);
      }
    },
  });
};

const resolveDialogSelectionPath = (selected) => {
  if (!selected) return '';
  if (Array.isArray(selected)) {
    return resolveDialogSelectionPath(selected[0]);
  }
  return selected.path || selected || '';
};

const getAssetFileName = (value) => String(value || '').split(/[\\/]/).pop() || '未选择资源';
const getDesktopPetAssetPreviewUrl = (node) => resolveDesktopPetAssetUrl(node?.src || '');
const selectedDesktopPetNode = computed(() => {
  const nodes = mainUiSettings.value?.desktopPet?.nodes || [];
  return nodes.find((node) => node.id === selectedDesktopPetNodeId.value) || nodes[0] || null;
});

const ensureSelectedDesktopPetNode = () => {
  const nodes = mainUiSettings.value?.desktopPet?.nodes || [];
  if (!nodes.length) {
    selectedDesktopPetNodeId.value = '';
    return;
  }
  if (!nodes.some((node) => node.id === selectedDesktopPetNodeId.value)) {
    selectedDesktopPetNodeId.value = nodes[0].id;
  }
};

const handleSelectDesktopPetNodeAsset = async (nodeId) => {
  try {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{
        name: '桌宠资源',
        extensions: ['png', 'webp', 'gif', 'jpg', 'jpeg', 'svg']
      }]
    });
    const path = resolveDialogSelectionPath(selected);
    if (!path) return;
    const imported = await invokeCommand('import_desktop_pet_asset', {
      sourcePath: path,
      actionKey: nodeId
    });
    if (!imported?.data_url) {
      throw new Error('导入结果缺少数据');
    }
    const targetNode = (mainUiSettings.value.desktopPet.nodes || []).find((node) => node.id === nodeId);
    if (!targetNode) return;
    // Use data_url for immediate display (bypasses asset protocol issues)
    targetNode.src = imported.data_url;
    targetNode.fileName = imported.file_name || getAssetFileName(path);
    targetNode.imported = true;
    targetNode.type = inferDesktopPetAssetType(imported.file_name || '');
    toast.success(`已导入资源：${targetNode.fileName}`);
  } catch (error) {
    toast.error(`资源导入失败：${error}`);
  }
};

const clearDesktopPetNodeAsset = (nodeId) => {
  const targetNode = (mainUiSettings.value.desktopPet.nodes || []).find((node) => node.id === nodeId);
  if (!targetNode) return;
  const defaults = defaultDesktopPetNodes[0];
  targetNode.src = defaults.src;
  targetNode.type = defaults.type;
  targetNode.imported = false;
  targetNode.fileName = defaults.fileName;
  targetNode.scale = defaults.scale;
  targetNode.offsetX = defaults.offsetX;
  targetNode.offsetY = defaults.offsetY;
  toast.success(`已恢复 ${targetNode.name || '节点'} 为默认资源`);
};

const addDesktopPetNode = () => {
  const nodes = mainUiSettings.value.desktopPet.nodes || [];
  const nextNode = createDefaultDesktopPetNode({
    id: `node-${Date.now()}`,
    name: `节点 ${nodes.length + 1}`
  });
  mainUiSettings.value.desktopPet.nodes = [...nodes, nextNode];
  selectedDesktopPetNodeId.value = nextNode.id;
};

const removeDesktopPetNode = (nodeId) => {
  const nodes = mainUiSettings.value.desktopPet.nodes || [];
  if (nodes.length <= 1) {
    toast.warning('至少保留一个节点');
    return;
  }
  const removed = nodes.find((node) => node.id === nodeId);
  mainUiSettings.value.desktopPet.nodes = nodes.filter((node) => node.id !== nodeId);
  ensureSelectedDesktopPetNode();
  if (removed) toast.success(`已删除节点「${removed.name || nodeId}」`);
};

const moveDesktopPetNode = (nodeId, direction) => {
  const nodes = [...(mainUiSettings.value.desktopPet.nodes || [])];
  const index = nodes.findIndex((node) => node.id === nodeId);
  if (index < 0) return;
  const nextIndex = index + direction;
  if (nextIndex < 0 || nextIndex >= nodes.length) return;
  const [targetNode] = nodes.splice(index, 1);
  nodes.splice(nextIndex, 0, targetNode);
  mainUiSettings.value.desktopPet.nodes = nodes;
  selectedDesktopPetNodeId.value = nodeId;
};

const reorderDesktopPetNode = (nodeId, targetNodeId) => {
  if (!nodeId || !targetNodeId || nodeId === targetNodeId) return;
  const nodes = [...(mainUiSettings.value.desktopPet.nodes || [])];
  const sourceIndex = nodes.findIndex((node) => node.id === nodeId);
  const targetIndex = nodes.findIndex((node) => node.id === targetNodeId);
  if (sourceIndex < 0 || targetIndex < 0) return;
  [nodes[sourceIndex], nodes[targetIndex]] = [nodes[targetIndex], nodes[sourceIndex]];
  mainUiSettings.value.desktopPet.nodes = nodes;
  selectedDesktopPetNodeId.value = nodeId;
};

const normalizeBindingKeyEvent = (e) => {
  const parts = [];
  if (e.ctrlKey) parts.push('Ctrl');
  if (e.shiftKey) parts.push('Shift');
  if (e.altKey) parts.push('Alt');
  if (e.metaKey) parts.push('Meta');

  const modifierOnlyKeys = ['Control', 'Shift', 'Alt', 'Meta'];
  if (modifierOnlyKeys.includes(e.key)) {
    return '';
  }

  let key = e.key === ' ' ? 'Space' : e.key;
  if (key === 'Esc') key = 'Escape';
  if (key.length === 1) key = key.toUpperCase();
  parts.push(key);
  return parts.join('+');
};

const onKeybindingInputFocus = (actionKey) => {
  bindingActionKey.value = actionKey;
};

const onKeybindingInputBlur = (actionKey) => {
  if (bindingActionKey.value === actionKey) {
    bindingActionKey.value = '';
  }
};

const onKeybindingInputKeydown = (actionKey, e) => {
  e.preventDefault();
  e.stopPropagation();
  bindingActionKey.value = actionKey;

  if (e.key === 'Backspace' || e.key === 'Delete') {
    commitBinding(actionKey, '');
    return;
  }

  if (e.key === 'Escape') {
    bindingActionKey.value = '';
    return;
  }

  const combo = normalizeBindingKeyEvent(e);
  if (!combo) return;
  commitBinding(actionKey, combo);
};

const onKeybindingInputMousedown = (actionKey, e) => {
  const combo = normalizeMouseBindingEvent(e);
  if (!combo) return;
  e.preventDefault();
  e.stopPropagation();
  bindingActionKey.value = actionKey;
  commitBinding(actionKey, combo);
};

// --- Security Lock Logic ---
async function setAppPassword() {
  if (lockPassword.value !== lockPasswordConfirm.value) {
    toast.error('两次输入的密码不一致');
    return;
  }
  if (!lockPassword.value) {
    if (securityStore.hasPassword) {
      await confirm({
        title: '确认移除密码？',
        content: '清除后，启动应用将不再需要验证。',
      });
      await securityStore.setPassword('');
      toast.success('密码已移除');
      return;
    }
  }

  await securityStore.setPassword(lockPassword.value);
  toast.success('应用密码设置成功');
}

async function verifyAndChange() {
  // Verify old password
  if (await securityStore.verifyPassword(currentLockPassword.value)) {
    await setAppPassword();
  } else {
    toast.error('当前密码验证失败');
  }
}
</script>

<template>
  <Dialog v-model:open="dialogOpen" modal>
    <DialogContent showCloseButton draggable
      class="flex h-[min(500px,calc(100vh-4rem))] max-h-[calc(100vh-4rem)] w-[740px] max-w-[90vw] flex-col sm:max-w-[90vw]">
      <DialogHeader>
        <DialogTitle>首选项</DialogTitle>
        <DialogDescription class="sr-only">配置应用的安全、外观、终端、快捷键等设置</DialogDescription>
      </DialogHeader>

      <div class="flex-1 min-h-0 flex overflow-hidden" :inert="closing || undefined">
        <div class="config-tab-sidebar flex flex-col gap-0.5 w-34 shrink-0 pr-2">
          <button v-for="tab in settingsTabs" :key="tab.key" type="button" :disabled="backgroundImporting"
            :class="['flex items-center gap-2 px-2 py-1.5 rounded text-sm text-left outline-none transition-[background,color,box-shadow]', 'focus-visible:[box-shadow:var(--app-focus-shadow)] disabled:pointer-events-none disabled:opacity-45', activeKey === tab.key ? 'bg-primary/15 text-primary font-semibold' : 'text-muted-foreground hover:bg-[var(--app-btn-hover)] hover:text-foreground active:bg-[var(--app-btn-pressed)]']"
            @click="activeKey = tab.key">
            <component :is="tab.icon" class="settings-tab-icon" aria-hidden="true" />
            {{ tab.label }}
          </button>
        </div>

        <div class="tab-content flex-1 min-h-0 overflow-auto px-4 py-2">
          <div v-if="aiPaneMounted" v-show="activeKey === 'ai'"><SettingsAiPane ref="aiPane" /></div>
          <SettingsSecurityPane v-if="activeKey === 'security'" :security-store="securityStore"
            :current-lock-password="currentLockPassword" :lock-password="lockPassword"
            :lock-password-confirm="lockPasswordConfirm" :set-app-password="setAppPassword"
            :verify-and-change="verifyAndChange" @update:current-lock-password="currentLockPassword = $event"
            @update:lock-password="lockPassword = $event"
            @update:lock-password-confirm="lockPasswordConfirm = $event" />

          <SettingsMainUiPane v-if="activeKey === 'main-ui'" :main-ui-settings="mainUiSettings"
            :selected-desktop-pet-node-id="selectedDesktopPetNodeId"
            :selected-desktop-pet-node="selectedDesktopPetNode"
            :get-desktop-pet-asset-file-name="getDesktopPetAssetFileName"
            :get-desktop-pet-asset-preview-url="getDesktopPetAssetPreviewUrl" :add-desktop-pet-node="addDesktopPetNode"
            :reorder-desktop-pet-node="reorderDesktopPetNode"
            :handle-select-desktop-pet-node-asset="handleSelectDesktopPetNodeAsset"
            :clear-desktop-pet-node-asset="clearDesktopPetNodeAsset" :remove-desktop-pet-node="removeDesktopPetNode"
            @update:selected-desktop-pet-node-id="selectedDesktopPetNodeId = $event"
            @background-imported="handleBackgroundImported"
            @background-preview-change="dispatchMainUiSettingsPreview"
            @background-importing="backgroundImporting = $event" />

          <SettingsTerminalPane v-if="activeKey === 'terminal'" :terminal-theme-settings="terminalThemeSettings"
            :command-history-settings="commandHistorySettings" :clear-command-history="clearCommandHistory"
            @preview-change="previewTerminalTheme" />

          <SettingsKeybindingsPane v-if="activeKey === 'keybindings'" :keybinding-items="keybindingItems"
            :keybindings="keybindings" :keybinding-conflict-map="keybindingConflictMap"
            :binding-action-key="bindingActionKey" :keybinding-conflict-entries="keybindingConflictEntries"
            :on-keybinding-input-focus="onKeybindingInputFocus" :on-keybinding-input-blur="onKeybindingInputBlur"
            :on-keybinding-input-keydown="onKeybindingInputKeydown"
            :on-keybinding-input-mousedown="onKeybindingInputMousedown" />
        </div>
      </div>


    </DialogContent>
  </Dialog>

</template>

<style scoped>
/* ── Settings modal layout ── */
.config-tab-sidebar {
  @apply pt-1;
}

.settings-tab-icon {
  width: 16px;
  height: 16px;
  flex-shrink: 0;
  stroke-width: 1.8;
}
</style>
