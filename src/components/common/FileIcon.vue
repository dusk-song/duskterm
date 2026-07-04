<script setup>
import {
  getIconForDirectoryPath,
  getIconForFilePath,
  getIconUrlByName,
  isMaterialIconName
} from 'vscode-material-icons';
import { computed, ref, watch } from 'vue';

const props = defineProps({
  name: { type: String, default: '' },
  path: { type: String, default: '' },
  isDirectory: { type: Boolean, default: false },
  expanded: { type: Boolean, default: false },
  size: { type: [Number, String], default: 16 }
});

const MATERIAL_ICONS_BASE = '/assets/material-icons';
const DEFAULT_FILE_ICON = 'file';
const DEFAULT_DIRECTORY_ICON = 'folder';
const DEFAULT_OPEN_DIRECTORY_ICON = 'folder-open';

const iconCache = new Map();
const iconLoadFailed = ref(false);

function normalizePath(value) {
  return String(value || '')
    .trim()
    .replace(/\\/g, '/')
    .replace(/\/+/g, '/');
}

function leafName(value) {
  const normalized = normalizePath(value);
  const segments = normalized.split('/');
  return segments[segments.length - 1] || normalized;
}

function safeIconName(value, fallbackName) {
  return isMaterialIconName(value) ? value : fallbackName;
}

function iconUrl(iconName) {
  return getIconUrlByName(iconName, MATERIAL_ICONS_BASE);
}

function resolveDirectoryIcon(path, expanded) {
  const baseIcon = safeIconName(getIconForDirectoryPath(path), DEFAULT_DIRECTORY_ICON);
  if (!expanded) return baseIcon;

  const expandedIcon = `${baseIcon}-open`;
  if (isMaterialIconName(expandedIcon)) return expandedIcon;
  return DEFAULT_OPEN_DIRECTORY_ICON;
}

function resolveIcon(name, path, isDirectory, expanded) {
  const lookupName = leafName(name || path);
  const cacheKey = `${isDirectory ? 'd' : 'f'}:${expanded ? '1' : '0'}:${lookupName}`;
  const cached = iconCache.get(cacheKey);
  if (cached) return cached;

  const fallbackName = isDirectory
    ? (expanded ? DEFAULT_OPEN_DIRECTORY_ICON : DEFAULT_DIRECTORY_ICON)
    : DEFAULT_FILE_ICON;
  const iconName = isDirectory
    ? resolveDirectoryIcon(lookupName, expanded)
    : safeIconName(getIconForFilePath(lookupName), DEFAULT_FILE_ICON);
  const result = {
    iconName: safeIconName(iconName, fallbackName),
    fallbackName
  };

  result.src = iconUrl(result.iconName);
  result.fallbackSrc = iconUrl(result.fallbackName);
  iconCache.set(cacheKey, result);
  return result;
}

const resolvedIcon = computed(() => resolveIcon(props.name, props.path, props.isDirectory, props.expanded));
const fallbackIcon = computed(() => resolvedIcon.value.fallbackSrc);
const iconSrc = computed(() => iconLoadFailed.value ? fallbackIcon.value : resolvedIcon.value.src);
const iconName = computed(() => iconLoadFailed.value ? resolvedIcon.value.fallbackName : resolvedIcon.value.iconName);
const iconSize = computed(() => typeof props.size === 'number' ? `${props.size}px` : props.size);

function handleIconError() {
  iconLoadFailed.value = true;
}

watch(() => [props.name, props.path, props.isDirectory, props.expanded], () => {
  iconLoadFailed.value = false;
});
</script>

<template>
  <span class="file-icon" :style="{ '--file-icon-size': iconSize }" :data-icon="iconName" aria-hidden="true">
    <img
      class="file-icon__img"
      :src="iconSrc"
      alt=""
      draggable="false"
      loading="lazy"
      decoding="async"
      @error="handleIconError"
    >
  </span>
</template>

<style scoped>
.file-icon {
  width: var(--file-icon-size, 16px);
  height: var(--file-icon-size, 16px);
  flex: 0 0 var(--file-icon-size, 16px);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}

.file-icon__img {
  width: 100%;
  height: 100%;
  flex: 0 0 auto;
  display: block;
  object-fit: contain;
  pointer-events: none;
  user-select: none;
}
</style>
