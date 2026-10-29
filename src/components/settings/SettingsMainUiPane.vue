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
import { GripVertical, HelpCircle, Plus, RefreshCw, Trash2, Upload, Video } from '@lucide/vue';
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
const backgroundPreviews = ref([]);
const videoThumbnailCache = new Map();
const activeVideoThumbnailCancels = new Set();
let backgroundPreviewToken = 0;
const displayTarget = () => {
  const scale = window.devicePixelRatio || 1;
  return { targetWidth: Math.round(window.screen.width * scale), targetHeight: Math.round(window.screen.height * scale) };
};

const ensureBackgroundSettings = () => {
  props.mainUiSettings.background = normalizeBackgroundSettings(props.mainUiSettings.background || {});
  return props.mainUiSettings.background;
};

const normalizedBackground = computed(() => normalizeBackgroundSettings(props.mainUiSettings.background || {}));
const isVideoBackground = computed(() => normalizedBackground.value.mediaType === 'video');

const captureVideoThumbnail = (url) => new Promise((resolve, reject) => {
  const video = document.createElement('video');
  let settled = false;
  let timeout = null;
  const cleanup = () => {
    if (timeout) clearTimeout(timeout);
    activeVideoThumbnailCancels.delete(cancel);
    video.pause();
    video.onloadeddata = null;
    video.onerror = null;
    video.removeAttribute('src');
    video.load();
  };
  const finish = (value, error = null) => {
    if (settled) return;
    settled = true;
    cleanup();
    if (error) reject(error);
    else resolve(value);
  };
  const cancel = () => finish(null, new Error('视频缩略图任务已取消'));
  activeVideoThumbnailCancels.add(cancel);
  timeout = setTimeout(() => finish(null, new Error('读取视频信息超时')), 12000);
  video.muted = true;
  video.crossOrigin = 'anonymous';
  video.playsInline = true;
  video.preload = 'auto';
  video.onloadeddata = () => {
    const width = video.videoWidth;
    const height = video.videoHeight;
    if (!width || !height) {
      finish(null, new Error('无法读取视频尺寸'));
      return;
    }
    let thumbnailUrl = '';
    try {
      const canvas = document.createElement('canvas');
      canvas.width = 320;
      canvas.height = 180;
      const scale = Math.max(canvas.width / width, canvas.height / height);
      const sourceWidth = canvas.width / scale;
      const sourceHeight = canvas.height / scale;
      const sourceX = (width - sourceWidth) / 2;
      const sourceY = (height - sourceHeight) / 2;
      canvas.getContext('2d')?.drawImage(
        video,
        sourceX,
        sourceY,
        sourceWidth,
        sourceHeight,
        0,
        0,
        canvas.width,
        canvas.height
      );
      thumbnailUrl = canvas.toDataURL('image/jpeg', 0.76);
    } catch {
      // The media remains usable even if the asset protocol disallows canvas extraction.
    }
    finish({ width, height, duration: Number(video.duration) || 0, thumbnailUrl });
  };
  video.onerror = () => finish(null, new Error('当前系统无法播放该视频，请使用 MP4(H.264) 或 WebM'));
  video.src = url;
  video.load();
});

const cancelVideoThumbnailCaptures = () => {
  for (const cancel of [...activeVideoThumbnailCancels]) cancel();
};

const pruneVideoThumbnailCache = (items) => {
  const retainedResourceIds = new Set(items.map((item) => item.resourceId));
  for (const resourceId of videoThumbnailCache.keys()) {
    if (!retainedResourceIds.has(resourceId)) videoThumbnailCache.delete(resourceId);
  }
};

const resolveBackgroundPreview = async (item) => {
  const asset = await invokeCommand('ensure_background_image', {
    resourceId: item.resourceId,
    ...displayTarget(),
  });
  const mediaType = asset.media_type === 'video' ? 'video' : 'image';
  const mediaUrl = resolveBackgroundUrl(asset.optimized_path);
  let thumbnailUrl = resolveBackgroundUrl(asset.thumbnail_path);
  if (mediaType === 'video' && mediaUrl) {
    const cached = videoThumbnailCache.get(item.resourceId);
    if (cached) thumbnailUrl = cached;
    else {
      try {
        const metadata = await captureVideoThumbnail(mediaUrl);
        thumbnailUrl = metadata.thumbnailUrl;
        if (thumbnailUrl) videoThumbnailCache.set(item.resourceId, thumbnailUrl);
      } catch {
        thumbnailUrl = '';
      }
    }
  }
  return {
    resourceId: item.resourceId,
    fileName: asset.file_name || item.fileName,
    mediaType,
    mediaUrl,
    thumbnailUrl,
    available: !!mediaUrl,
  };
};

const refreshBackgroundPreviews = async () => {
  const token = ++backgroundPreviewToken;
  const items = normalizedBackground.value.recentAssets;
  cancelVideoThumbnailCaptures();
  pruneVideoThumbnailCache(items);
  const previews = [];
  for (const item of items) {
    try {
      previews.push(await resolveBackgroundPreview(item));
    } catch {
      previews.push({ ...item, mediaUrl: '', thumbnailUrl: '', available: false });
    }
    if (token !== backgroundPreviewToken) return;
  }
  backgroundPreviews.value = previews;
};

const notifyBackgroundPreviewChange = () => {
  emit('background-preview-change');
};

const selectBackgroundImage = async () => {
  const selected = await open({
    multiple: false,
    filters: [{ name: '背景媒体', extensions: ['png', 'jpg', 'jpeg', 'webp', 'mp4', 'webm'] }]
  });
  if (!selected) return;
  backgroundPreviewToken += 1;
  cancelVideoThumbnailCaptures();
  const sourcePath = typeof selected === 'string' ? selected : selected.path;
  isImportingBackground.value = true;
  emit('background-importing', true);
  let importedResourceId = '';
  try {
    const asset = await invokeCommand('import_background_image', {
      sourcePath,
      ...displayTarget(),
    });
    importedResourceId = asset.resource_id;
    emit('background-imported', asset.resource_id);
    const mediaType = asset.media_type === 'video' ? 'video' : 'image';
    const mediaUrl = resolveBackgroundUrl(asset.optimized_path);
    let thumbnailUrl = resolveBackgroundUrl(asset.thumbnail_path);
    if (mediaType === 'video') {
      const metadata = await captureVideoThumbnail(mediaUrl);
      if (metadata.width * metadata.height > 3840 * 2160) {
        throw new Error('视频分辨率不能超过 3840×2160，推荐使用 1080p/30fps');
      }
      thumbnailUrl = metadata.thumbnailUrl;
      if (thumbnailUrl) videoThumbnailCache.set(asset.resource_id, thumbnailUrl);
    }
    const recentAsset = {
      resourceId: asset.resource_id,
      fileName: asset.file_name,
      mediaType,
    };
    props.mainUiSettings.background = normalizeBackgroundSettings({
      ...props.mainUiSettings.background,
      enabled: true,
      ...recentAsset,
      fit: mediaType === 'video' && props.mainUiSettings.background?.fit === 'tile'
        ? 'cover'
        : props.mainUiSettings.background?.fit,
      recentAssets: [recentAsset, ...(props.mainUiSettings.background?.recentAssets || [])],
    });
    backgroundPreviews.value = [{ ...recentAsset, mediaUrl, thumbnailUrl, available: true }, ...backgroundPreviews.value]
      .filter((item, index, list) => list.findIndex((entry) => entry.resourceId === item.resourceId) === index)
      .slice(0, 3);
    importedResourceId = '';
    notifyBackgroundPreviewChange();
  } catch (error) {
    if (importedResourceId) {
      try { await invokeCommand('delete_background_image', { resourceId: importedResourceId }); } catch { /* best-effort cleanup */ }
    }
    toast.error(`背景媒体导入失败：${error}`);
  }
  finally { isImportingBackground.value = false; emit('background-importing', false); }
};

const removeBackgroundImage = () => {
  const current = normalizedBackground.value;
  const recentAssets = current.recentAssets.filter((asset) => asset.resourceId !== current.resourceId);
  const next = recentAssets[0] || null;
  props.mainUiSettings.background = normalizeBackgroundSettings({
    ...current,
    enabled: !!next,
    resourceId: next?.resourceId || '',
    fileName: next?.fileName || '',
    mediaType: next?.mediaType || 'image',
    fit: next?.mediaType === 'video' && current.fit === 'tile' ? 'cover' : current.fit,
    recentAssets,
  });
  notifyBackgroundPreviewChange();
};

const selectRecentBackground = (item) => {
  if (!item?.resourceId || !item.available) return;
  const current = normalizedBackground.value;
  const selected = current.recentAssets.find((asset) => asset.resourceId === item.resourceId);
  if (!selected) return;
  props.mainUiSettings.background = normalizeBackgroundSettings({
    ...current,
    ...selected,
    enabled: true,
    fit: selected.mediaType === 'video' && current.fit === 'tile' ? 'cover' : current.fit,
    recentAssets: [selected, ...current.recentAssets.filter((asset) => asset.resourceId !== selected.resourceId)],
  });
  notifyBackgroundPreviewChange();
};

ensureBackgroundSettings();
watch(() => [
  props.mainUiSettings.background?.enabled,
  props.mainUiSettings.background?.resourceId,
  props.mainUiSettings.background?.fit,
  props.mainUiSettings.background?.blur,
  props.mainUiSettings.background?.opacity,
  props.mainUiSettings.background?.darkOverlay,
  props.mainUiSettings.background?.lightOverlay
], notifyBackgroundPreviewChange);
watch(
  () => normalizedBackground.value.recentAssets
    .map((asset) => `${asset.resourceId}:${asset.mediaType}`)
    .join('|'),
  () => { void refreshBackgroundPreviews(); },
  { immediate: true }
);

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
  backgroundPreviewToken += 1;
  cancelVideoThumbnailCaptures();
  videoThumbnailCache.clear();
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

    <div class="settings-section idea-panel">
      <div class="settings-section-title-wrap">
        <div class="settings-section-title">最近会话</div>
      </div>
      <div class="setting-row">
        <div class="setting-label">显示列表</div>
        <Switch v-model="mainUiSettings.recentSessions.enabled" />
      </div>
      <div class="setting-row">
        <div class="setting-label">保留数量</div>
        <Input v-model="mainUiSettings.recentSessions.limit" inputmode="numeric" class="recent-session-limit"
          :disabled="!mainUiSettings.recentSessions.enabled" />
        <span class="setting-value">最多 100</span>
      </div>
    </div>

    <div class="settings-section idea-panel background-settings">
      <div class="settings-section-title-wrap"><div class="settings-section-title">全局背景</div></div>
      <div v-if="backgroundPreviews.length" class="background-recent" aria-label="最近使用的背景">
        <Tooltip v-for="item in backgroundPreviews" :key="item.resourceId">
          <TooltipTrigger as-child>
            <button type="button" class="background-recent-item"
              :class="{ active: item.resourceId === normalizedBackground.resourceId, unavailable: !item.available }"
              :aria-disabled="isImportingBackground || !item.available"
              @click="!isImportingBackground && selectRecentBackground(item)">
              <span class="background-recent-thumb"
                :style="item.thumbnailUrl ? { backgroundImage: `url(${item.thumbnailUrl})` } : null">
                <Video v-if="item.mediaType === 'video' && !item.thumbnailUrl" />
              </span>
              <span class="background-recent-name">{{ item.fileName || '背景媒体' }}</span>
              <span v-if="item.mediaType === 'video'" class="background-media-badge">视频</span>
            </button>
          </TooltipTrigger>
          <TooltipContent>{{ item.fileName || '背景媒体' }}</TooltipContent>
        </Tooltip>
      </div>
      <div class="setting-row"><div class="setting-label">启用背景</div><Switch v-model="mainUiSettings.background.enabled" :disabled="!normalizedBackground.resourceId" /></div>
      <div class="setting-row background-actions"><div class="setting-label">媒体文件</div><span class="background-file">{{ mainUiSettings.background.fileName || '无' }}</span><Button size="sm" variant="outline" :disabled="isImportingBackground" @click="selectBackgroundImage">{{ isImportingBackground ? '处理中…' : '选择媒体' }}</Button><Button size="sm" variant="outline" :disabled="!normalizedBackground.resourceId" @click="removeBackgroundImage">移除</Button></div>
      <div class="setting-row"><div class="setting-label">铺放方式</div><Select v-model="mainUiSettings.background.fit"><SelectTrigger size="sm" class="background-select"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="cover">填充</SelectItem><SelectItem value="contain">适应</SelectItem><SelectItem value="stretch">拉伸</SelectItem><SelectItem value="center">居中</SelectItem><SelectItem value="tile" :disabled="isVideoBackground">平铺</SelectItem></SelectContent></Select></div>
      <div class="setting-row"><div class="setting-label">模糊度</div><Slider v-model="mainUiSettings.background.blur" :min="0" :max="40" :step="1" class="line-slider" /><span class="setting-value">{{ mainUiSettings.background.blur }}px</span></div>
      <div class="setting-row"><div class="setting-label">背景透明度</div><Slider v-model="mainUiSettings.background.opacity" :min="0" :max="1" :step="0.05" class="line-slider" /><span class="setting-value">{{ Math.round(mainUiSettings.background.opacity * 100) }}%</span></div>
      <div class="setting-row"><div class="setting-label">暗色遮罩</div><Slider v-model="mainUiSettings.background.darkOverlay" :min="0" :max="0.9" :step="0.05" class="line-slider" /><span class="setting-value">{{ Math.round(mainUiSettings.background.darkOverlay * 100) }}%</span></div>
      <div class="setting-row"><div class="setting-label">亮色遮罩</div><Slider v-model="mainUiSettings.background.lightOverlay" :min="0" :max="0.9" :step="0.05" class="line-slider" /><span class="setting-value">{{ Math.round(mainUiSettings.background.lightOverlay * 100) }}%</span></div>
    </div>

    <div class="settings-section idea-panel">
      <div class="settings-section-title-wrap">
        <div class="settings-section-title">小狗桌宠</div>
        <Tooltip>
          <TooltipTrigger>
            <HelpCircle class="section-tip-icon" />
          </TooltipTrigger>
          <TooltipContent>
            桌面宠物动画，支持自定义节点和资源；点击穿透开启时按住 Alt 拖动，关闭时直接拖动；后台和调整布局时暂停播放。
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

.recent-session-limit {
  @apply w-20;
}

.background-recent { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; margin: 8px 0 12px; }
.background-recent-item { position: relative; min-width: 0; padding: 4px; overflow: hidden; border: 1px solid transparent; border-radius: 8px; background: color-mix(in srgb, var(--app-input-bg) 88%, transparent); color: var(--app-text-muted); text-align: left; cursor: pointer; }
.background-recent-item:hover { border-color: color-mix(in srgb, var(--color-primary) 55%, var(--app-border-shadow)); }
.background-recent-item.active { border-color: var(--color-primary); }
.background-recent-item.unavailable { opacity: 0.55; cursor: not-allowed; }
.background-recent-thumb { display: flex; width: 100%; aspect-ratio: 16 / 9; align-items: center; justify-content: center; overflow: hidden; border-radius: 5px; background: color-mix(in srgb, var(--app-bg-dialog) 82%, #000); background-position: center; background-size: cover; }
.background-recent-thumb svg { width: 22px; height: 22px; }
.background-recent-name { display: block; padding: 4px 2px 1px; overflow: hidden; color: var(--app-text); font-size: 11px; line-height: 16px; text-overflow: ellipsis; white-space: nowrap; }
.background-media-badge { position: absolute; top: 8px; right: 8px; padding: 1px 5px; border-radius: 999px; background: rgba(0, 0, 0, 0.68); color: #fff; font-size: 9px; line-height: 15px; }
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
  border: 1px solid transparent;
  border-radius: 14px;
  background: transparent;
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
  border: 1px solid transparent;
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
  border: 1px solid transparent;
  border-radius: 12px;
  background: transparent;
}

.pet-asset-item-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding-bottom: 6px;
  border-bottom: 1px solid transparent;
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
  background: transparent;
  border: 1px solid transparent;
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
  border: 1px solid transparent;
  background: transparent;
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
