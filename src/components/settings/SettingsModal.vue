<script setup>
import Button from '@/components/ui/button/Button.vue';
import Dialog from '@/components/ui/dialog/Dialog.vue';
import DialogContent from '@/components/ui/dialog/DialogContent.vue';
import DialogDescription from '@/components/ui/dialog/DialogDescription.vue';
import DialogFooter from '@/components/ui/dialog/DialogFooter.vue';
import DialogHeader from '@/components/ui/dialog/DialogHeader.vue';
import DialogTitle from '@/components/ui/dialog/DialogTitle.vue';
import { confirm } from '@/composables/useConfirm';
import { toast } from '@/composables/useToast';
import {
  Home,
  Keyboard,
  Lock,
  Settings2,
  Terminal
} from '@lucide/vue';
import { open } from '@tauri-apps/plugin-dialog';
import { computed, defineAsyncComponent, ref, watch } from 'vue';
import { useSecurityStore } from '@/stores/security';
import {
  createDefaultDesktopPetNode,
  defaultDesktopPetNodes,
  getDesktopPetAssetFileName,
  inferDesktopPetAssetType,
  resolveDesktopPetAssetUrl
} from '@/utils/desktopPet';
import { invokeCommand } from '@/utils/ipc';
import { loadLightbarSettings, saveLightbarSettings } from '@/utils/lightbar';
import { loadMainUiSettings, saveMainUiSettings } from '@/utils/mainUi';
import { loadMonitorSettings, saveMonitorSettings } from '@/utils/monitor';
import { getPreferenceDefaults, loadPreference, savePreference } from '@/utils/preferences';
import { loadTerminalThemeSettings, saveTerminalThemeSettings } from '@/utils/terminalTheme';

// Lazy-loaded sub-panes — only the visible tab's code is loaded, reducing first-open lag
const SettingsKeybindingsPane = defineAsyncComponent(() => import('./SettingsKeybindingsPane.vue'));
const SettingsMainUiPane = defineAsyncComponent(() => import('./SettingsMainUiPane.vue'));
const SettingsSecurityPane = defineAsyncComponent(() => import('./SettingsSecurityPane.vue'));
const SettingsStatusBarPane = defineAsyncComponent(() => import('./SettingsStatusBarPane.vue'));
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
const activeKey = ref('security');

const settingsTabs = [
  { key: 'security', label: '安全锁', icon: Lock },
  { key: 'main-ui', label: '应用', icon: Home },
  { key: 'statusbar', label: '状态栏', icon: Settings2 },
  { key: 'terminal', label: '终端', icon: Terminal },
  { key: 'keybindings', label: '快捷键', icon: Keyboard },
];
const lightbarSettings = ref(loadLightbarSettings());
const monitorSettings = ref(loadMonitorSettings());
const terminalThemeSettings = ref(loadTerminalThemeSettings());
const mainUiSettings = ref(loadMainUiSettings());
const selectedDesktopPetNodeId = ref('');

const defaultKeybindings = getPreferenceDefaults('keybindings');

const keybindingItems = [
  { key: 'splitHorizontal', label: '水平拆分', placeholder: 'Ctrl+Shift+U' },
  { key: 'splitVertical', label: '垂直拆分', placeholder: 'Ctrl+Alt+I' },
  { key: 'closeSession', label: '关闭当前会话', placeholder: 'Ctrl+Shift+W' },
  { key: 'nextSession', label: '下一个会话', placeholder: 'Ctrl+Tab' },
  { key: 'prevSession', label: '上一个会话', placeholder: 'Ctrl+Shift+Tab' },
  { key: 'sessionList', label: '会话列表', placeholder: 'Ctrl+Alt+1' },
  { key: 'sftpPanel', label: '文件管理', placeholder: 'Ctrl+Alt+2' },
  { key: 'commandKnowledge', label: '命令知识库', placeholder: 'Ctrl+Alt+3' },
  { key: 'overview', label: '总览模式', placeholder: 'Ctrl+`' },
  { key: 'copySession', label: '复制当前会话', placeholder: 'Ctrl+P' },
  { key: 'syncMergedPrevPage', label: '合并视图上一页', placeholder: 'Ctrl+Shift+PageUp' },
  { key: 'syncMergedNextPage', label: '合并视图下一页', placeholder: 'Ctrl+Shift+PageDown' },
  { key: 'toggleLineNumbers', label: '切换行号', placeholder: 'Ctrl+Alt+L' },
  { key: 'toggleFind', label: '终端搜索', placeholder: 'Ctrl+Shift+F' }
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

// Sync when opening
watch(() => props.visible, (val) => {
  if (val) {
    // Reset password fields
    lockPassword.value = '';
    lockPasswordConfirm.value = '';
    currentLockPassword.value = '';
    try {
      keybindings.value = { ...defaultKeybindings, ...loadPreference('keybindings') };
    } catch (e) {
      keybindings.value = { ...defaultKeybindings };
    }
    lightbarSettings.value = loadLightbarSettings();
    monitorSettings.value = loadMonitorSettings();
    terminalThemeSettings.value = loadTerminalThemeSettings();
    mainUiSettings.value = loadMainUiSettings();
    ensureSelectedDesktopPetNode();
  } else {
    bindingActionKey.value = '';
  }
});

const handleSave = async () => {
  if (keybindingConflictEntries.value.length > 0) {
    const conflictText = keybindingConflictEntries.value
      .map((entry) => `${entry.labels.join('、')}（${entry.combo}）`)
      .join('；');
    activeKey.value = 'keybindings';
    toast.error(`快捷键存在冲突：${conflictText}`);
    return;
  }

  try {
    savePreference('keybindings', keybindings.value);
    window.dispatchEvent(new CustomEvent('keybindings-changed'));
  } catch (e) {
    toast.error('快捷键保存失败');
  }
  try {
    saveLightbarSettings(lightbarSettings.value);
    window.dispatchEvent(new CustomEvent('lightbar-settings-changed'));
  } catch (e) {
    toast.error('灯条设置保存失败');
  }
  try {
    saveMonitorSettings(monitorSettings.value);
    window.dispatchEvent(new CustomEvent('monitor-settings-changed'));
  } catch (e) {
    toast.error('监控设置保存失败');
  }
  try {
    saveTerminalThemeSettings(terminalThemeSettings.value);
  } catch (e) {
    toast.error('终端样式保存失败');
  }
  try {
    saveMainUiSettings(mainUiSettings.value);
    window.dispatchEvent(new CustomEvent('main-ui-settings-changed'));
  } catch (e) {
    toast.error('主界面设置保存失败');
  }
  emit('update:visible', false);
  toast.success('设置已保存');
};

const handleCancel = () => {
  emit('update:visible', false);
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
    keybindings.value[actionKey] = '';
    return;
  }

  if (e.key === 'Escape') {
    bindingActionKey.value = '';
    return;
  }

  const combo = normalizeBindingKeyEvent(e);
  if (!combo) return;
  keybindings.value[actionKey] = combo;
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
    <DialogContent showCloseButton
      class="flex h-[min(500px,calc(100vh-4rem))] max-h-[calc(100vh-4rem)] w-[740px] max-w-[90vw] flex-col sm:max-w-[90vw]">
      <DialogHeader>
        <DialogTitle>首选项</DialogTitle>
        <DialogDescription class="sr-only">配置应用的安全、外观、终端、快捷键等设置</DialogDescription>
      </DialogHeader>

      <div class="flex-1 min-h-0 flex overflow-hidden">
        <div class="config-tab-sidebar flex flex-col gap-0.5 w-34 shrink-0 pr-2 border-r border-border">
          <button v-for="tab in settingsTabs" :key="tab.key" type="button"
            :class="['flex items-center gap-2 px-2 py-1.5 rounded text-sm text-left outline-none transition-[background,color,box-shadow]', 'focus-visible:bg-[var(--app-focus-bg)] focus-visible:text-foreground focus-visible:shadow-[var(--app-focus-shadow)]', activeKey === tab.key ? 'bg-primary/15 text-primary font-semibold' : 'text-muted-foreground hover:text-foreground']"
            @click="activeKey = tab.key">
            <component :is="tab.icon" class="settings-tab-icon" aria-hidden="true" />
            {{ tab.label }}
          </button>
        </div>

        <div class="tab-content flex-1 min-h-0 overflow-auto px-4 py-2">
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
            @update:selected-desktop-pet-node-id="selectedDesktopPetNodeId = $event" />

          <SettingsStatusBarPane v-if="activeKey === 'statusbar'" :lightbar-settings="lightbarSettings"
            :monitor-settings="monitorSettings" />

          <SettingsTerminalPane v-if="activeKey === 'terminal'" :terminal-theme-settings="terminalThemeSettings" />

          <SettingsKeybindingsPane v-if="activeKey === 'keybindings'" :keybinding-items="keybindingItems"
            :keybindings="keybindings" :keybinding-conflict-map="keybindingConflictMap"
            :binding-action-key="bindingActionKey" :keybinding-conflict-entries="keybindingConflictEntries"
            :on-keybinding-input-focus="onKeybindingInputFocus" :on-keybinding-input-blur="onKeybindingInputBlur"
            :on-keybinding-input-keydown="onKeybindingInputKeydown" />
        </div>
      </div>

      <DialogFooter>
        <Button variant="outline" size="sm" @click="handleCancel">取消</Button>
        <Button size="sm" @click="handleSave">保存</Button>
      </DialogFooter>
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
