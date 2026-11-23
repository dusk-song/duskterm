<script setup>
import { computed, ref, watch } from 'vue';
import { Copy } from '@lucide/vue';
import { Button } from '@/components/ui/button';
import { TooltipHint } from '@/components/ui/tooltip';
import SettingsSelect from '@/components/settings/SettingsSelect.vue';
import { toast } from '@/composables/useToast';
import { invokeCommand } from '@/utils/ipc';
import { decodeBytes, decodeText, decompressGzip, formatJson, hex } from './redisBytes';

const props = defineProps({ value: { type: Object, required: true }, hideSize: Boolean, fill: Boolean });
const mode = defineModel({ type: String, default: 'auto' });
const modes = [
  ['auto', '自动'], ['utf8', 'UTF-8'], ['json', 'JSON'], ['hex', '十六进制'],
  ['base64', 'Base64 表示'], ['base64-decode', 'Base64 解码'], ['msgpack', 'MessagePack'], ['gzip', 'Gzip 解压'],
].map(([value, label]) => ({ value, label }));
const content = ref('');
const label = ref('');
const warning = ref(false);
const hexMode = ref(false);
const jsonMode = ref(false);
const previewRef = ref(null);
const pending = ref(false);
const rawBytes = computed(() => decodeBytes(props.value.base64));
let current = 0;

const jsonParts = computed(() => {
  const text = content.value;
  if (!jsonMode.value || text.length > 8192) return null;
  const parts = [];
  let offset = 0;
  for (const match of text.matchAll(/("(?:\\.|[^"\\])*")(\s*:)?|-?\d+(?:\.\d+)?(?:e[+-]?\d+)?|\b(?:true|false|null)\b/gi)) {
    if (parts.length >= 512) return null;
    if (match.index > offset) parts.push({ text: text.slice(offset, match.index) });
    parts.push({ text: match[0], tone: match[1] ? (match[2] === undefined ? 'string' : 'key') : 'literal' });
    offset = match.index + match[0].length;
  }
  if (offset < text.length) parts.push({ text: text.slice(offset) });
  return parts;
});

watch([() => props.value, mode], async ([value, format], [previousValue, previousFormat], onCleanup) => {
  const revision = ++current;
  onCleanup(() => { current += 1; });
  pending.value = true;
  const raw = rawBytes.value;
  let text;
  let name = modes.find(option => option.value === format)?.label || format;
  let displayHex = format === 'hex';
  let fallback = false;
  try {
    switch (format) {
      case 'base64': text = value.base64; break;
      case 'hex': text = hex(raw); break;
      case 'utf8': text = decodeText(raw); break;
      case 'json': text = formatJson(decodeText(raw)); break;
      case 'base64-decode': text = decodeText(decodeBytes(decodeText(raw))); break;
      case 'gzip': text = await decompressGzip(raw); break;
      case 'msgpack': text = await invokeCommand('redis_preview_msgpack', { data: value.base64 }); break;
      default: {
        text = decodeText(raw);
        if (/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/.test(text)) throw new Error('二进制内容');
        name = 'UTF-8';
        if (/^\s*[\[{]/.test(text)) {
          try { text = formatJson(text); name = 'JSON'; } catch { /* Keep the original text. */ }
        }
      }
    }
  } catch {
    text = hex(raw);
    name = format === 'auto' ? '二进制 · 十六进制' : name + ' 无法预览，已显示原始字节';
    fallback = format !== 'auto';
    displayHex = true;
  }
  if (revision !== current) return;
  content.value = text;
  label.value = raw.length === value.length ? name : name + ' · 已截断';
  warning.value = fallback;
  hexMode.value = displayHex;
  jsonMode.value = !displayHex && name === 'JSON';
  pending.value = false;
  if (fallback && previousValue === value && previousFormat !== format) toast.warning(name);
}, { immediate: true });

async function copyPreview() {
  try {
    await navigator.clipboard.writeText(content.value);
    toast.success('已复制 Redis 值');
  } catch {
    const selection = window.getSelection();
    if (!previewRef.value || !selection) {
      toast.error('无法访问剪贴板');
      return;
    }
    const range = document.createRange();
    range.selectNodeContents(previewRef.value);
    selection.removeAllRanges();
    selection.addRange(range);
    toast.warning('无法访问剪贴板，已选中值内容，可手动复制');
  }
}
</script>

<template>
  <section class="min-w-0 gap-2" :class="fill ? 'flex min-h-0 flex-1 flex-col' : 'grid'">
    <div class="flex shrink-0 flex-wrap items-center gap-2 text-xs text-muted-foreground">
      <span class="mr-auto font-semibold text-foreground">值</span>
      <span v-if="!hideSize">{{ value.length }} B</span>
      <SettingsSelect item-class="text-xs" v-model="mode" aria-label="查看方式" class="!w-32" :options="modes" />
      <TooltipHint text="复制值">
        <Button variant="ghost" size="icon-sm" aria-label="复制值" :disabled="pending" @click="copyPreview"><Copy /></Button>
      </TooltipHint>
    </div>
    <pre ref="previewRef" class="min-w-0 overflow-auto rounded-lg border border-border bg-[var(--app-input-bg)] p-3 font-mono text-xs leading-relaxed"
      :class="[fill ? 'min-h-0 flex-1' : 'min-h-32 max-h-[46vh]', hexMode ? 'whitespace-pre' : 'whitespace-pre-wrap break-all']"><template v-if="jsonParts"><template v-for="(part, i) in jsonParts" :key="i"><span v-if="part.tone" :data-tone="part.tone">{{ part.text }}</span><template v-else>{{ part.text }}</template></template></template><template v-else>{{ content || '（空值）' }}</template></pre>
    <p role="status" class="shrink-0 text-xs" :class="warning ? 'text-destructive' : 'text-muted-foreground'">{{ pending ? '解析中…' : '当前视图 · ' + label }}</p>
  </section>
</template>

<style scoped>
[data-tone="key"] { color: var(--app-risk-warning); }
[data-tone="string"] { color: var(--app-toast-success); }
[data-tone="literal"] { color: var(--app-toast-info); }
</style>
