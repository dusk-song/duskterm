<script setup>
import Button from '@/components/ui/button/Button.vue';
import Input from '@/components/ui/input/Input.vue';
import Select from '@/components/ui/select/Select.vue';
import SelectContent from '@/components/ui/select/SelectContent.vue';
import SelectItem from '@/components/ui/select/SelectItem.vue';
import SelectTrigger from '@/components/ui/select/SelectTrigger.vue';
import SelectValue from '@/components/ui/select/SelectValue.vue';
import Slider from '@/components/ui/slider/Slider.vue';
import Switch from '@/components/ui/switch/Switch.vue';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { useTheme } from '@/composables/useTheme';
import { toast } from '@/composables/useToast';
import { invokeCommand } from '@/utils/ipc';
import { normalizeBackgroundSettings, resolveBackgroundUrl } from '@/utils/background';
import { open } from '@tauri-apps/plugin-dialog';
import { GripVertical, HelpCircle, Plus, RefreshCw, Trash2, Upload } from '@lucide/vue';
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';

const { isDark, toggleTheme, isFollowingSystem, followSystem, setTheme } = useTheme();

const props = defineProps({
  mainUiSettings: {
    type: Object,
    required: true
  },
  selectedDesktopPetNodeId: {
    type: String,
    default: ''
  },
  selectedDesktopPetNode: {
    type: Object,
    default: null
  },
  getDesktopPetAssetFileName: {
    type: Function,
    required: true
  },
  getDesktopPetAssetPreviewUrl: {
    type: Function,
    required: true
  },
  addDesktopPetNode: {
    type: Function,
    required: true
  },
  reorderDesktopPetNode: {
    type: Function,
    required: true
  },
  handleSelectDesktopPetNodeAsset: {
    type: Function,
    required: true
  },
  clearDesktopPetNodeAsset: {
    type: Function,
    required: true
  },
  removeDesktopPetNode: {
    type: Function,
    required: true
  }
});

const emit = defineEmits(['update:selectedDesktopPetNodeId', 'background-imported', 'background-preview-change', 'background-importing']);

const editingNodeId = ref('');
const editingNodeName = ref('');
const editingInputRef = ref(null);
const draggingNodeId = ref('');
const dragOverNodeId = ref('');
const isPointerDragging = ref(false);
const isImportingBackground = ref(false);
const backgroundPreview = ref('');
const displayTarget = () => {
  const scale = window.devicePixelRatio || 1;
  return { targetWidth: Math.round(window.screen.width * scale), targetHeight: Math.round(window.screen.height * scale) };
};
const refreshBackgroundPreview = async (resourceId) => {
  if (!resourceId) { backgroundPreview.value = ''; return; }
  try {
    const asset = await invokeCommand('ensure_background_image', { resourceId, ...displayTarget() });
    backgroundPreview.value = resolveBackgroundUrl(asset.optimized_path);
  } catch { backgroundPreview.value = ''; }
};

const ensureBackgroundSettings = () => {
  props.mainUiSettings.background = normalizeBackgroundSettings(props.mainUiSettings.background || {});
  return props.mainUiSettings.background;
};

const notifyBackgroundPreviewChange = () => {
  emit('background-preview-change');
};

const selectBackgroundImage = async () => {
  const selected = await open({ multiple: false, filters: [{ name: '静态图片', extensions: ['png', 'jpg', 'jpeg', 'webp'] }] });
  if (!selected) return;
  const sourcePath = typeof selected === 'string' ? selected : selected.path;
  isImportingBackground.value = true;
  emit('background-importing', true);
  try {
    const asset = await invokeCommand('import_background_image', {
      sourcePath,
      ...displayTarget(),
    });
    props.mainUiSettings.background = normalizeBackgroundSettings({
      ...props.mainUiSettings.background,
      enabled: true,
      resourceId: asset.resource_id,
      fileName: asset.file_name,
    });
    backgroundPreview.value = resolveBackgroundUrl(asset.optimized_path);
    emit('background-imported', asset.resource_id);
    notifyBackgroundPreviewChange();
  } catch (error) { toast.error(`背景图片导入失败：${error}`); }
  finally { isImportingBackground.value = false; emit('background-importing', false); }
};

const removeBackgroundImage = () => {
  props.mainUiSettings.background = normalizeBackgroundSettings();
  backgroundPreview.value = '';
  notifyBackgroundPreviewChange();
};

ensureBackgroundSettings();
watch(() => props.mainUiSettings.background?.resourceId, refreshBackgroundPreview, { immediate: true });
watch(() => [
  props.mainUiSettings.background?.blur,
  props.mainUiSettings.background?.opacity,
  props.mainUiSettings.background?.darkOverlay,
  props.mainUiSettings.background?.lightOverlay
], notifyBackgroundPreviewChange);

const getNodeDisplayName = (node, index) => node?.name?.trim() || `节点 ${index + 1}`;

const beginRenameNode = async (node, index) => {
  editingNodeId.value = node.id;
  editingNodeName.value = getNodeDisplayName(node, index);
  emit('update:selectedDesktopPetNodeId', node.id);
  await nextTick();
  const inputInstance = editingInputRef.value;
  inputInstance?.focus?.();
  inputInstance?.select?.();
};

const commitRenameNode = (node, index) => {
  if (editingNodeId.value !== node.id) return;
  node.name = editingNodeName.value.trim() || `节点 ${index + 1}`;
  editingNodeId.value = '';
  editingNodeName.value = '';
};

const cancelRenameNode = () => {
  editingNodeId.value = '';
  editingNodeName.value = '';
};

const findNodeIdFromPoint = (clientX, clientY) => {
  const element = document.elementFromPoint(clientX, clientY);
  const nodeElement = element?.closest?.('[data-pet-node-id]');
  return nodeElement?.getAttribute?.('data-pet-node-id') || '';
};

const resetDraggingState = () => {
  draggingNodeId.value = '';
  dragOverNodeId.value = '';
  isPointerDragging.value = false;
  document.removeEventListener('mousemove', handlePointerDragMove);
  document.removeEventListener('mouseup', handlePointerDragEnd);
};

const handlePointerDragStart = (nodeId, event) => {
  if (event.button !== 0) return;
  draggingNodeId.value = nodeId;
  dragOverNodeId.value = nodeId;
  isPointerDragging.value = false;
  if (editingNodeId.value) {
    cancelRenameNode();
  }
  document.addEventListener('mousemove', handlePointerDragMove);
  document.addEventListener('mouseup', handlePointerDragEnd);
};

const handlePointerDragMove = (event) => {
  if (!draggingNodeId.value) return;
  isPointerDragging.value = true;
  const targetNodeId = findNodeIdFromPoint(event.clientX, event.clientY);
  if (!targetNodeId || targetNodeId === draggingNodeId.value) return;
  dragOverNodeId.value = targetNodeId;
};

const handlePointerDragEnd = (event) => {
  if (!draggingNodeId.value) return;
  const sourceNodeId = draggingNodeId.value;
  const targetNodeId = findNodeIdFromPoint(event.clientX, event.clientY) || dragOverNodeId.value;
  if (isPointerDragging.value && targetNodeId && targetNodeId !== sourceNodeId) {
    props.reorderDesktopPetNode(sourceNodeId, targetNodeId);
  }
  resetDraggingState();
};

onBeforeUnmount(() => {
  resetDraggingState();
});
</script>

<template>
  <div class="settings-content">
    <div class="settings-section idea-panel">
      <div class="settings-section-title-wrap">
        <div class="settings-section-title">主题</div>
        <Tooltip>
          <TooltipTrigger>
            <HelpCircle class="section-tip-icon" />
          </TooltipTrigger>
          <TooltipContent>
            切换亮色/暗色主题，支持跟随系统主题设置。
          </TooltipContent>
        </Tooltip>
      </div>
      <div class="setting-row">
        <div class="setting-label">暗色模式</div>
        <Switch :model-value="isDark" @update:model-value="(v) => setTheme(v ? 'dark' : 'light')" />
      </div>
      <div class="setting-row">
        <div class="setting-label">跟随系统</div>
        <Switch :model-value="isFollowingSystem"
          @update:model-value="(v) => { if (v) followSystem(); else setTheme(isDark ? 'dark' : 'light'); }" />
      </div>
    </div>

    <div class="settings-section idea-panel background-settings">
      <div class="settings-section-title-wrap"><div class="settings-section-title">全局背景图片</div></div>
      <div class="background-preview" :class="{ empty: !backgroundPreview }"
        :style="backgroundPreview ? { backgroundImage: `url(${backgroundPreview})` } : null">
        <span v-if="!backgroundPreview">未选择背景图片</span>
      </div>
      <div class="setting-row"><div class="setting-label">启用背景</div><Switch v-model="mainUiSettings.background.enabled" :disabled="!backgroundPreview" /></div>
      <div class="setting-row background-actions"><div class="setting-label">图片文件</div><span class="background-file">{{ mainUiSettings.background.fileName || '无' }}</span><Button size="sm" variant="outline" :disabled="isImportingBackground" @click="selectBackgroundImage">{{ isImportingBackground ? '处理中…' : '选择图片' }}</Button><Button size="sm" variant="outline" :disabled="!backgroundPreview" @click="removeBackgroundImage">移除</Button></div>
      <div class="setting-row"><div class="setting-label">铺放方式</div><Select v-model="mainUiSettings.background.fit"><SelectTrigger size="sm" class="background-select"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="cover">填充</SelectItem><SelectItem value="contain">适应</SelectItem><SelectItem value="stretch">拉伸</SelectItem><SelectItem value="center">居中</SelectItem><SelectItem value="tile">平铺</SelectItem></SelectContent></Select></div>
      <div class="setting-row"><div class="setting-label">模糊度</div><Slider v-model="mainUiSettings.background.blur" :min="0" :max="40" :step="1" class="line-slider" /><span class="setting-value">{{ mainUiSettings.background.blur }}px</span></div>
      <div class="setting-row"><div class="setting-label">图片透明度</div><Slider v-model="mainUiSettings.background.opacity" :min="0" :max="1" :step="0.05" class="line-slider" /><span class="setting-value">{{ Math.round(mainUiSettings.background.opacity * 100) }}%</span></div>
      <div class="setting-row"><div class="setting-label">暗色遮罩</div><Slider v-model="mainUiSettings.background.darkOverlay" :min="0" :max="0.9" :step="0.05" class="line-slider" /><span class="setting-value">{{ Math.round(mainUiSettings.background.darkOverlay * 100) }}%</span></div>
      <div class="setting-row"><div class="setting-label">亮色遮罩</div><Slider v-model="mainUiSettings.background.lightOverlay" :min="0" :max="0.9" :step="0.05" class="line-slider" /><span class="setting-value">{{ Math.round(mainUiSettings.background.lightOverlay * 100) }}%</span></div>
    </div>

    <div class="settings-section idea-panel">
      <div class="settings-section-title">内容显示</div>
      <div class="setting-row">
        <div class="setting-label">显示贪吃蛇游戏</div>
        <Switch v-model="mainUiSettings.showSnakeGame" />
      </div>
    </div>

    <div class="settings-section idea-panel">
      <div class="settings-section-title-wrap">
        <div class="settings-section-title">小狗桌宠</div>
        <Tooltip>
          <TooltipTrigger>
            <HelpCircle class="section-tip-icon" />
          </TooltipTrigger>
          <TooltipContent>
            桌面宠物动画，支持自定义节点和资源。
          </TooltipContent>
        </Tooltip>
      </div>
      <div class="setting-row">
        <div class="setting-label">启用桌宠</div>
        <Switch v-model="mainUiSettings.desktopPet.enabled" />
      </div>
      <div class="setting-row">
        <div class="setting-label">宠物尺寸</div>
        <Slider v-model="mainUiSettings.desktopPet.scale" :min="0.6" :max="1.8" :step="0.1" class="line-slider" />
        <span class="setting-value">{{ Number(mainUiSettings.desktopPet.scale).toFixed(1) }}x</span>
      </div>
      <div class="setting-row">
        <div class="setting-label">透明度</div>
        <Slider v-model="mainUiSettings.desktopPet.opacity" :min="0.45" :max="1" :step="0.05" class="line-slider" />
        <span class="setting-value">{{ Math.round(Number(mainUiSettings.desktopPet.opacity) * 100) }}%</span>
      </div>
      <div class="setting-row">
        <div class="setting-label">点击穿透</div>
        <Switch v-model="mainUiSettings.desktopPet.clickThrough" />
      </div>
      <div class="setting-row">
        <div class="setting-label">弹窗时隐藏</div>
        <Switch v-model="mainUiSettings.desktopPet.autoHideOnModal" />
      </div>
      <div class="setting-row">
        <div class="setting-label">边缘探测</div>
        <Switch v-model="mainUiSettings.desktopPet.edgeProbeEnabled" />
      </div>
      <div class="setting-row">
        <div class="setting-label">探测范围</div>
        <Slider v-model="mainUiSettings.desktopPet.edgeProbeMargin" :min="8" :max="120" :step="2" class="line-slider" />
        <span class="setting-value">{{ Number(mainUiSettings.desktopPet.edgeProbeMargin).toFixed(0) }}px</span>
      </div>
      <div class="setting-row">
        <div class="setting-label">上边缘节点</div>
        <Select v-model="mainUiSettings.desktopPet.edgeProbeNodeTop">
          <SelectTrigger size="sm" class="setting-select">
            <SelectValue placeholder="默认(不切换)" />
          </SelectTrigger>
          <SelectContent position="popper" side="bottom" align="start" :side-offset="4" :collision-padding="16">
            <SelectItem value="__none__">默认(不切换)</SelectItem>
            <SelectItem v-for="node in mainUiSettings.desktopPet.nodes" :key="node.id" :value="node.id">{{ node.name ||
              '未命名' }}</SelectItem>
          </SelectContent>
        </Select>
      </div>
      <div class="setting-row">
        <div class="setting-label">右边缘节点</div>
        <Select v-model="mainUiSettings.desktopPet.edgeProbeNodeRight">
          <SelectTrigger size="sm" class="setting-select">
            <SelectValue placeholder="默认(不切换)" />
          </SelectTrigger>
          <SelectContent position="popper" side="bottom" align="start" :side-offset="4" :collision-padding="16">
            <SelectItem value="__none__">默认(不切换)</SelectItem>
            <SelectItem v-for="node in mainUiSettings.desktopPet.nodes" :key="node.id" :value="node.id">{{ node.name ||
              '未命名' }}</SelectItem>
          </SelectContent>
        </Select>
      </div>
      <div class="setting-row">
        <div class="setting-label">下边缘节点</div>
        <Select v-model="mainUiSettings.desktopPet.edgeProbeNodeBottom">
          <SelectTrigger size="sm" class="setting-select">
            <SelectValue placeholder="默认(不切换)" />
          </SelectTrigger>
          <SelectContent position="popper" side="bottom" align="start" :side-offset="4" :collision-padding="16">
            <SelectItem value="__none__">默认(不切换)</SelectItem>
            <SelectItem v-for="node in mainUiSettings.desktopPet.nodes" :key="node.id" :value="node.id">{{ node.name ||
              '未命名' }}</SelectItem>
          </SelectContent>
        </Select>
      </div>
      <div class="setting-row">
        <div class="setting-label">左边缘节点</div>
        <Select v-model="mainUiSettings.desktopPet.edgeProbeNodeLeft">
          <SelectTrigger size="sm" class="setting-select">
            <SelectValue placeholder="默认(不切换)" />
          </SelectTrigger>
          <SelectContent position="popper" side="bottom" align="start" :side-offset="4" :collision-padding="16">
            <SelectItem value="__none__">默认(不切换)</SelectItem>
            <SelectItem v-for="node in mainUiSettings.desktopPet.nodes" :key="node.id" :value="node.id">{{ node.name ||
              '未命名' }}</SelectItem>
          </SelectContent>
        </Select>
      </div>
    </div>

    <div class="settings-section idea-panel">
      <div class="settings-section-title-wrap">
        <div class="settings-section-title">桌宠节点</div>
        <Tooltip>
          <TooltipTrigger>
            <HelpCircle class="section-tip-icon" />
          </TooltipTrigger>
          <TooltipContent>
            左侧拖拽调整顺序，双击节点名称可重命名；右侧仅保留预览和显示参数。
          </TooltipContent>
        </Tooltip>
      </div>

      <div class="pet-asset-layout">
        <div class="pet-asset-sidebar">
          <div class="pet-asset-sidebar-head">
            <div>
              <div class="pet-asset-sidebar-title">节点列表</div>
              <div class="pet-asset-sidebar-subtitle">共 {{ mainUiSettings.desktopPet.nodes.length }} 个节点</div>
            </div>
            <Button size="sm" @click="addDesktopPetNode">
              <Plus :size="14" /> 添加
            </Button>
          </div>

          <div class="pet-asset-nav-list" role="listbox" aria-label="桌宠节点列表">
            <div v-for="(node, index) in mainUiSettings.desktopPet.nodes" :key="node.id" class="pet-asset-nav-item"
              :data-pet-node-id="node.id" :class="{
                active: selectedDesktopPetNodeId === node.id,
                imported: node.imported,
                disabled: node.enabled === false,
                dragging: draggingNodeId === node.id,
                'drag-over': dragOverNodeId === node.id && draggingNodeId !== node.id
              }" :aria-selected="selectedDesktopPetNodeId === node.id">
              <button v-if="editingNodeId !== node.id" type="button" class="pet-asset-nav-button"
                @click="emit('update:selectedDesktopPetNodeId', node.id)" @dblclick.stop="beginRenameNode(node, index)">
                <div class="pet-asset-nav-main">
                  <span class="pet-asset-drag-handle" @mousedown.prevent.stop="handlePointerDragStart(node.id, $event)">
                    <GripVertical :size="14" />
                  </span>
                  <span class="pet-asset-nav-order">{{ index + 1 }}</span>
                  <span class="pet-asset-nav-label">{{ getNodeDisplayName(node, index) }}</span>
                </div>
                <span class="pet-asset-nav-state">{{ node.enabled ? '启用' : '停用' }}</span>
              </button>

              <div v-else class="pet-asset-nav-editing">
                <span class="pet-asset-nav-order">{{ index + 1 }}</span>
                <Input ref="editingInputRef" v-model="editingNodeName" :maxlength="32" size="sm" class="w-full"
                  @keyup.enter="commitRenameNode(node, index)" @blur="commitRenameNode(node, index)"
                  @keydown.esc="cancelRenameNode" />
              </div>
            </div>
          </div>
        </div>

        <div v-if="selectedDesktopPetNode" class="pet-asset-item">
          <div class="pet-asset-item-head">
            <div class="pet-asset-meta">
              <Tooltip>
                <TooltipTrigger>
                  <Switch v-model="selectedDesktopPetNode.enabled" />
                </TooltipTrigger>
                <TooltipContent>
                  启用/禁用节点
                </TooltipContent>
              </Tooltip>
            </div>

            <div class="pet-asset-actions">
              <Button size="sm" variant="outline"
                @click="() => handleSelectDesktopPetNodeAsset(selectedDesktopPetNode.id)">
                <Upload :size="12" /> 导入
              </Button>
              <Button size="sm" variant="outline" @click="() => clearDesktopPetNodeAsset(selectedDesktopPetNode.id)">
                <RefreshCw :size="12" /> 恢复
              </Button>
              <Button size="sm" variant="destructive" @click="() => removeDesktopPetNode(selectedDesktopPetNode.id)">
                <Trash2 :size="12" /> 删除
              </Button>
            </div>
          </div>

          <div class="pet-asset-content">
            <div class="pet-preview-panel">
              <div class="pet-asset-preview-card" :class="{ empty: !selectedDesktopPetNode.src }">
                <img v-if="selectedDesktopPetNode.src" class="pet-asset-preview-image"
                  :src="getDesktopPetAssetPreviewUrl(selectedDesktopPetNode)"
                  :alt="selectedDesktopPetNode.name || '桌宠节点'" />
                <div v-else class="pet-asset-preview-empty">未配置资源</div>
              </div>
            </div>

            <div class="pet-inspector-section">
              <div class="pet-inspector-title">显示参数</div>
              <div class="flex flex-col gap-2">
                <div class="form-item">
                  <label>停留时长</label>
                  <div class="form-item-control">
                    <Input v-model="selectedDesktopPetNode.durationMs" :min="200" :max="60000" :step="100" size="sm"
                      class="w-full" />
                  </div>
                </div>
                <div class="form-item">
                  <label>尺寸</label>
                  <div class="form-item-control">
                    <Input v-model="selectedDesktopPetNode.scale" :min="0.4" :max="2.4" :step="0.1" size="sm"
                      class="w-full" />
                  </div>
                </div>
                <div class="form-item">
                  <label>X 偏移</label>
                  <div class="form-item-control">
                    <Input v-model="selectedDesktopPetNode.offsetX" :min="-120" :max="120" :step="2" size="sm"
                      class="w-full" />
                  </div>
                </div>
                <div class="form-item">
                  <label>Y 偏移</label>
                  <div class="form-item-control">
                    <Input v-model="selectedDesktopPetNode.offsetY" :min="-120" :max="120" :step="2" size="sm"
                      class="w-full" />
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
@import './settingsPaneShared.css';

.background-preview { height: 120px; margin: 8px 0 12px; border: 1px solid var(--app-border-shadow); border-radius: 8px; background-position: center; background-size: cover; }
.background-preview.empty { display: flex; align-items: center; justify-content: center; color: var(--app-text-muted); background: var(--app-input-bg); }
.background-actions { gap: 8px; }
.background-file { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--app-text-muted); font-size: 12px; }
.background-select { width: 140px; }


.pet-asset-layout {
  display: grid;
  grid-template-columns: 200px 1fr;
  gap: 16px;
  align-items: start;
}

.pet-asset-sidebar,
.pet-asset-item,
.pet-inspector-section {
  min-width: 0;
}

.pet-asset-sidebar {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px;
  border: 1px solid var(--app-border-shadow);
  border-radius: 14px;
  background: color-mix(in srgb, var(--app-input-bg) 80%, transparent);
  height: fit-content;
}

.pet-asset-sidebar-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.pet-asset-sidebar-title {
  color: var(--mac-text-secondary);
  font-size: 11px;
  font-weight: 600;
}

.pet-asset-sidebar-subtitle {
  margin-top: 2px;
  color: var(--mac-text-secondary);
  font-size: 11px;
  opacity: 0.82;
}

.pet-asset-nav-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 420px;
  overflow: auto;
  padding-right: 2px;
}

.pet-asset-nav-item {
  width: 100%;
  min-width: 0;
}

.pet-asset-nav-button,
.pet-asset-nav-editing {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  width: 100%;
  min-width: 0;
  padding: 8px 10px;
  border-radius: 8px;
  border: 1px solid var(--app-border-shadow);
  background: color-mix(in srgb, var(--app-input-bg) 70%, transparent);
  color: var(--mac-text-primary);
  text-align: left;
}

.pet-asset-nav-button {
  cursor: pointer;
  transition: border-color 0.12s ease, background 0.12s ease, transform 0.12s ease;
}

.pet-asset-nav-button:hover {
  transform: translateY(-1px);
  border-color: color-mix(in srgb, var(--app-text) 16%, var(--app-border-shadow));
  background: color-mix(in srgb, var(--app-input-bg) 88%, transparent);
}

.pet-asset-nav-button:focus-visible {
  outline: none;
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-primary) 35%, transparent);
}

.pet-asset-nav-item.active .pet-asset-nav-button,
.pet-asset-nav-item.active .pet-asset-nav-editing {
  border-color: color-mix(in srgb, var(--color-success) 48%, var(--app-border-shadow));
  background: color-mix(in srgb, var(--color-success) 8%, transparent);
}

.pet-asset-nav-item.imported:not(.active) .pet-asset-nav-button,
.pet-asset-nav-item.imported:not(.active) .pet-asset-nav-editing {
  border-color: color-mix(in srgb, var(--color-primary) 28%, var(--app-border-shadow));
}

.pet-asset-nav-item.dragging {
  opacity: 0.64;
}

.pet-asset-nav-item.drag-over .pet-asset-nav-button,
.pet-asset-nav-item.drag-over .pet-asset-nav-editing {
  border-color: color-mix(in srgb, var(--color-primary) 50%, var(--app-border-shadow));
  background: color-mix(in srgb, var(--color-primary) 10%, transparent);
}

.pet-asset-nav-main {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.pet-asset-drag-handle {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  flex-shrink: 0;
  color: var(--mac-text-secondary);
  opacity: 0.8;
  cursor: grab;
}

.pet-asset-drag-handle:hover {
  opacity: 1;
}

.pet-asset-nav-item.dragging .pet-asset-drag-handle {
  cursor: grabbing;
}

.pet-asset-nav-order {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  flex-shrink: 0;
  border-radius: 6px;
  background: color-mix(in srgb, var(--app-input-bg) 90%, var(--app-bg-dialog));
  color: var(--mac-text-secondary);
  font-size: 11px;
  font-weight: 600;
}

.pet-asset-nav-label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
  font-weight: 600;
  line-height: 1.15;
}

.pet-asset-nav-state {
  flex-shrink: 0;
  margin-left: 8px;
  color: var(--mac-text-secondary);
  font-size: 11px;
}

.pet-asset-nav-editing :deep(input) {
  @apply flex-1 min-w-0;
}

.pet-asset-nav-item.disabled .pet-asset-nav-order,
.pet-asset-nav-item.disabled .pet-asset-nav-label,
.pet-asset-nav-item.disabled .pet-asset-nav-state {
  opacity: 0.65;
}

.pet-asset-item {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 12px;
  border: 1px solid var(--app-border-shadow);
  border-radius: 12px;
  background: linear-gradient(
    180deg,
    color-mix(in srgb, var(--app-input-bg) 88%, transparent),
    color-mix(in srgb, var(--app-bg-dialog) 96%, transparent)
  );
}

.pet-asset-item-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--app-border-shadow);
}

.pet-asset-meta {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.pet-asset-meta strong {
  color: var(--mac-text-primary);
  font-size: 12px;
}

.pet-asset-meta span {
  color: var(--mac-text-secondary);
  font-size: 11px;
}

.pet-asset-actions {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.pet-asset-content {
  display: flex;
  gap: 16px;
  align-items: start;
}

.pet-preview-panel {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  flex-shrink: 0;
  width: 96px;
}

.pet-asset-preview-card {
  width: 96px;
  height: 96px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 10px;
  background: color-mix(in srgb, var(--app-input-bg) 86%, transparent);
  border: 1px solid var(--app-border-shadow);
  overflow: hidden;
}

.pet-asset-preview-card.empty {
  border-style: dashed;
}

.pet-asset-preview-image {
  width: 100%;
  height: 100%;
  object-fit: contain;
  padding: 4px;
}

.pet-asset-preview-empty {
  color: var(--mac-text-secondary);
  font-size: 11px;
  text-align: center;
  padding: 0 8px;
}

.pet-asset-field-card {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
  padding: 10px 12px;
  border-radius: 10px;
  background: color-mix(in srgb, var(--app-input-bg) 78%, transparent);
  border: 1px solid var(--app-border-shadow);
}

.pet-asset-field-card span {
  color: var(--mac-text-secondary);
  font-size: 11px;
}

.pet-asset-field-card--switch {
  flex: 1;
  flex-direction: row;
  align-items: center;
  justify-content: space-between;
}

.pet-inspector-section {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px;
  border-radius: 12px;
  border: 1px solid var(--app-border-shadow);
  background: color-mix(in srgb, var(--app-input-bg) 70%, transparent);
}

.pet-inspector-title {
  @apply text-xs font-semibold text-[var(--app-text)] mb-1;
}

/* 显示参数表单 — 左对齐标签 */
.pet-inspector-section .form-item {
  @apply flex items-center gap-2 min-h-0 mb-0;
}

.pet-inspector-section .form-item label {
  @apply w-[60px] shrink-0 text-left text-[11px] text-[var(--app-text-muted)];
}

.pet-asset-help-icon {
  @apply text-[11px] text-[var(--app-text-muted)] opacity-80 cursor-help;
}

@media (max-width: 768px) {
  .pet-asset-layout {
    @apply grid-cols-1;
  }

  .pet-asset-item-head,
  .pet-preview-panel {
    @apply flex-col items-start;
  }

  .pet-asset-actions {
    @apply justify-start;
  }
}
</style>
