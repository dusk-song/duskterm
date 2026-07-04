import { convertFileSrc } from '@tauri-apps/api/core';

const defaultDesktopPetAssetSrc = '/pets/work1.gif';

const legacyActionPresets = [
  { key: 'idle', label: '待机' },
  { key: 'run', label: '奔跑' },
  { key: 'walk', label: '行走' },
  { key: 'slow-walk', label: '慢走' },
  { key: 'sniff', label: '嗅探' },
  { key: 'jump', label: '跳跃' },
  { key: 'sit', label: '坐下' },
  { key: 'happy', label: '开心' },
  { key: 'nap', label: '打盹' },
  { key: 'turn', label: '转向' }
];

const defaultDesktopPetNodes = [
  {
    id: 'node-1',
    name: '默认节点',
    src: defaultDesktopPetAssetSrc,
    type: 'gif',
    imported: false,
    fileName: 'work1.gif',
    durationMs: 2000,
    scale: 0.7,
    offsetX: 0,
    offsetY: 0,
    enabled: true
  }
];

function clampNodeScale(value, fallback = 0.7) {
  const nextValue = Number(value);
  if (!Number.isFinite(nextValue)) return fallback;
  return Math.min(2.4, Math.max(0.4, nextValue));
}

function clampNodeOffset(value, fallback = 0) {
  const nextValue = Number(value);
  if (!Number.isFinite(nextValue)) return fallback;
  return Math.min(240, Math.max(-240, nextValue));
}

function clampNodeDuration(value, fallback = 2000) {
  const nextValue = Number(value);
  if (!Number.isFinite(nextValue)) return fallback;
  return Math.min(30000, Math.max(200, Math.round(nextValue)));
}

function clampDesktopPetTop(value, fallback = 0.01) {
  const nextValue = Number(value);
  if (!Number.isFinite(nextValue)) return fallback;
  // Support both legacy px (>1) and new fraction (0-1)
  if (nextValue > 1) return Math.min(1, Math.max(0, nextValue / 1200));
  return Math.min(0.98, Math.max(0, nextValue));
}

function clampDesktopPetRight(value, fallback = 0.01) {
  const nextValue = Number(value);
  if (!Number.isFinite(nextValue)) return fallback;
  if (nextValue > 1) return Math.min(1, Math.max(0, nextValue / 1920));
  return Math.min(0.98, Math.max(0, nextValue));
}

function inferDesktopPetAssetType(src = '') {
  const value = String(src || '').trim().toLowerCase();
  if (value.endsWith('.gif')) return 'gif';
  if (value.endsWith('.png') || value.endsWith('.webp') || value.endsWith('.jpg') || value.endsWith('.jpeg') || value.endsWith('.svg')) {
    return 'image';
  }
  return 'builtin';
}

function getDesktopPetAssetFileName(src = '') {
  return String(src || '').split(/[\\/]/).pop() || '未命名资源';
}

function resolveDesktopPetAssetUrl(src = '') {
  const value = String(src || '').trim();
  if (!value) return '';
  if (/^(https?:|data:|blob:|asset:|tauri:|file:|\/)/i.test(value)) return value;
  if (/^[A-Za-z]:[\\/]/.test(value) || value.startsWith('\\\\')) {
    const normalizedPath = value.replace(/\//g, '\\');
    try {
      return convertFileSrc(normalizedPath);
    } catch {
      const normalized = normalizedPath.replace(/\\/g, '/');
      return normalized.startsWith('//') ? encodeURI(`file:${normalized}`) : encodeURI(`file:///${normalized}`);
    }
  }
  return value;
}

function normalizeDesktopPetNode(node = {}, index = 0) {
  const fallback = defaultDesktopPetNodes[Math.min(index, defaultDesktopPetNodes.length - 1)] || defaultDesktopPetNodes[0];
  const src = String(node.src || fallback.src || defaultDesktopPetAssetSrc).trim() || defaultDesktopPetAssetSrc;
  const type = ['image', 'gif', 'builtin'].includes(node.type) ? node.type : inferDesktopPetAssetType(src);
  return {
    id: String(node.id || `node-${index + 1}`),
    name: String(node.name || fallback.name || `节点 ${index + 1}`).trim() || `节点 ${index + 1}`,
    src,
    type: type === 'builtin' ? inferDesktopPetAssetType(src) : type,
    imported: !!node.imported,
    fileName: String(node.fileName || getDesktopPetAssetFileName(src) || fallback.fileName || ''),
    durationMs: clampNodeDuration(node.durationMs, fallback.durationMs || 2000),
    scale: clampNodeScale(node.scale, fallback.scale || 0.7),
    offsetX: clampNodeOffset(node.offsetX, fallback.offsetX || 0),
    offsetY: clampNodeOffset(node.offsetY, fallback.offsetY || 0),
    enabled: node.enabled !== false
  };
}

function createDefaultDesktopPetNode(overrides = {}) {
  return normalizeDesktopPetNode({
    ...defaultDesktopPetNodes[0],
    id: overrides.id || `node-${Date.now()}`,
    ...overrides
  });
}

function deriveLegacyNodes(settings = {}) {
  const actionAssets = settings.actionAssets || {};
  const legacyPath = Array.isArray(settings.path) ? settings.path : [];
  const nodes = [];

  legacyPath.forEach((point, index) => {
    const candidateKeys = [point.arrivalAction, point.action, point.travelAction].filter(Boolean);
    const matchedKey = candidateKeys.find((key) => actionAssets[key]?.src) || candidateKeys[0] || '';
    const matchedLabel = legacyActionPresets.find((action) => action.key === matchedKey)?.label || `节点 ${index + 1}`;
    const asset = actionAssets[matchedKey] || {};
    const src = String(asset.src || defaultDesktopPetAssetSrc).trim() || defaultDesktopPetAssetSrc;

    nodes.push(normalizeDesktopPetNode({
      id: point.id || `legacy-node-${index + 1}`,
      name: matchedLabel,
      src,
      type: inferDesktopPetAssetType(src),
      imported: !!asset.imported,
      fileName: asset.fileName || getDesktopPetAssetFileName(src),
      durationMs: point.arrivalDurationMs ?? point.pauseMs ?? 2000,
      scale: asset.scale ?? 1,
      offsetX: asset.offsetX ?? 0,
      offsetY: asset.offsetY ?? 0,
      enabled: true
    }, index));
  });

  if (nodes.length > 0) return nodes;

  const configuredAssets = legacyActionPresets
    .map((action) => ({ action, asset: actionAssets[action.key] || null }))
    .filter(({ asset }) => asset && asset.src);

  if (configuredAssets.length > 0) {
    return configuredAssets.map(({ action, asset }, index) => normalizeDesktopPetNode({
      id: `legacy-asset-${index + 1}`,
      name: action.label,
      src: asset.src,
      type: inferDesktopPetAssetType(asset.src),
      imported: !!asset.imported,
      fileName: asset.fileName || getDesktopPetAssetFileName(asset.src),
      durationMs: 2000,
      scale: asset.scale ?? 1,
      offsetX: asset.offsetX ?? 0,
      offsetY: asset.offsetY ?? 0,
      enabled: true
    }, index));
  }

  return defaultDesktopPetNodes.map((node, index) => normalizeDesktopPetNode(node, index));
}

function normalizeDesktopPetNodes(nodes, settings = {}) {
  if (Array.isArray(nodes) && nodes.length > 0) {
    return nodes.map((node, index) => normalizeDesktopPetNode(node, index));
  }
  return deriveLegacyNodes(settings);
}

const defaultDesktopPetSettings = {
  enabled: true,
  scale: 1,
  opacity: 0.96,
  clickThrough: true,
  autoHideOnModal: true,
  edgeProbeEnabled: true,
  edgeProbeMargin: 28,
  edgeProbeNodeTop: '__none__',
  edgeProbeNodeRight: '__none__',
  edgeProbeNodeBottom: '__none__',
  edgeProbeNodeLeft: '__none__',
  transitionMs: 220,
  positionTop: 0.01,
  positionRight: 0.01,
  nodes: defaultDesktopPetNodes
};

function normalizeDesktopPetSettings(settings = {}) {
  const next = {
    ...defaultDesktopPetSettings,
    ...(settings || {})
  };

  next.enabled = next.enabled !== false;
  next.scale = Math.min(1.8, Math.max(0.6, Number(next.scale || 1)));
  next.opacity = Math.min(1, Math.max(0.45, Number(next.opacity || 1)));
  next.transitionMs = Math.min(2000, Math.max(80, Number(next.transitionMs || 220)));
  next.clickThrough = next.clickThrough !== false;
  next.autoHideOnModal = next.autoHideOnModal !== false;
  next.edgeProbeEnabled = next.edgeProbeEnabled !== false;
  next.edgeProbeMargin = Math.min(120, Math.max(8, Number(next.edgeProbeMargin || defaultDesktopPetSettings.edgeProbeMargin)));
  next.edgeProbeNodeTop = String(next.edgeProbeNodeTop || '__none__').trim();
  next.edgeProbeNodeRight = String(next.edgeProbeNodeRight || '__none__').trim();
  next.edgeProbeNodeBottom = String(next.edgeProbeNodeBottom || '__none__').trim();
  next.edgeProbeNodeLeft = String(next.edgeProbeNodeLeft || '__none__').trim();
  next.positionTop = clampDesktopPetTop(next.positionTop, defaultDesktopPetSettings.positionTop);
  next.positionRight = clampDesktopPetRight(next.positionRight, defaultDesktopPetSettings.positionRight);
  next.nodes = normalizeDesktopPetNodes(next.nodes, settings || {});

  return next;
}

function createDefaultDesktopPetSettings(overrides = {}) {
  return normalizeDesktopPetSettings({
    ...defaultDesktopPetSettings,
    ...overrides
  });
}

export {
  createDefaultDesktopPetNode,
  createDefaultDesktopPetSettings,
  defaultDesktopPetAssetSrc,
  defaultDesktopPetNodes,
  defaultDesktopPetSettings,
  getDesktopPetAssetFileName,
  inferDesktopPetAssetType,
  normalizeDesktopPetNode,
  normalizeDesktopPetNodes,
  normalizeDesktopPetSettings,
  resolveDesktopPetAssetUrl
};

