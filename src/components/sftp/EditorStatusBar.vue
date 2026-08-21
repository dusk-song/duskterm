<script setup>
import { formatLocalTime, formatPermissions, formatSize } from '@/types/sftp';
import { reactive } from 'vue';

defineProps({
  meta: { type: Object, required: true },
  language: { type: String, default: 'plaintext' },
  encoding: { type: String, default: 'utf-8' },
  lineEnding: { type: String, default: 'lf' },
  readonly: { type: Boolean, default: false }
});

const cursor = reactive({ line: 1, column: 1 });

function setCursor(position) {
  cursor.line = Number(position?.line || 1);
  cursor.column = Number(position?.column || 1);
}

defineExpose({ setCursor });
</script>

<template>
  <div class="editor-statusbar">
    <span class="editor-status-item">大小 {{ formatSize(meta.size || 0) }}</span>
    <span class="editor-status-item">修改 {{ formatLocalTime(meta.modified || 0) }}</span>
    <span class="editor-status-item">权限 {{ formatPermissions(meta.permissions || 0) }}</span>
    <span class="editor-status-item">用户 {{ meta.owner || '-' }}/{{ meta.group || '-' }}</span>
    <span class="editor-status-item">语言 {{ language }}</span>
    <span class="editor-status-item">编码 {{ encoding }}</span>
    <span class="editor-status-item">换行 {{ lineEnding.toUpperCase() }}</span>
    <span class="editor-status-item">行 {{ cursor.line }}，列 {{ cursor.column }}</span>
    <span v-if="readonly" class="editor-status-item">只读</span>
  </div>
</template>

<style scoped>
.editor-statusbar {
  min-height: 34px;
  padding: 6px 12px;
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  border-top: 1px solid var(--app-border-shadow, rgba(255, 255, 255, 0.08));
  color: var(--app-text-muted);
  font-size: 12px;
  font-family: var(--app-font-family);
  background: color-mix(in srgb, var(--app-bg-dialog) 97%, var(--app-text));
}

.editor-status-item {
  display: inline-flex;
  align-items: center;
  min-height: 24px;
  padding: 0 8px;
  border-radius: var(--niri-radius-sm, 4px);
  background: color-mix(in srgb, var(--app-text-muted, #ABB2BF) 10%, transparent);
  color: var(--app-text-muted);
  font-size: 11px;
}
</style>
