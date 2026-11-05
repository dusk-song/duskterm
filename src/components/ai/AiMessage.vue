<script setup>
import { Copy, RotateCcw } from '@lucide/vue';
import { Button } from '@/components/ui/button';
import IconButton from '@/components/common/IconButton.vue';
import { formatTokens } from '@/utils/ai';
import { toast } from '@/composables/useToast';
import AiMarkdown from './AiMarkdown.vue';

defineProps({ message: Object, canRegenerate: Boolean, busy: Boolean });
defineEmits(['regenerate', 'select']);
async function copy(text) {
  try { await navigator.clipboard.writeText(text); toast.success('已复制'); }
  catch { toast.error('复制失败，请手动选择文本'); }
}
</script>

<template>
  <article class="ai-message" :class="[message.role, { alternative: !message.active && message.role === 'assistant' }]">
    <div class="bubble">
      <span v-if="message.role === 'assistant' && !message.active && message.status === 'completed'" class="version">历史版本</span>
      <details v-if="message.reasoning" class="thinking"><summary>{{ message.status === 'streaming' ? '思考中' : '思考过程' }}</summary><pre>{{ message.reasoning }}</pre></details>
      <div v-if="message.role === 'user'" class="user-content">{{ message.content }}</div>
      <AiMarkdown v-else :content="message.content" :streaming="message.status === 'streaming'" />
      <p v-if="message.status === 'streaming'" role="status" class="muted">正在生成…</p>
      <p v-if="['stopped', 'interrupted', 'failed'].includes(message.status)" class="message-error">{{ message.error || '回答未完成，不参与后续上下文' }}</p>
    </div>
    <div class="actions">
      <IconButton :icon="Copy" size="sm" aria-label="复制" tooltip-side="top" :action="() => copy(message.content)" />
      <IconButton v-if="canRegenerate" :icon="RotateCcw" size="sm" aria-label="重新生成" tooltip-side="top" :disabled="busy" :action="() => $emit('regenerate')" />
      <span v-if="message.role === 'assistant' && message.status !== 'streaming'" class="usage">输入 {{ formatTokens(message.usage?.inputTokens) }} · 输出 {{ formatTokens(message.usage?.outputTokens) }} · 总计 {{ formatTokens(message.usage?.totalTokens) }} Token · {{ ((message.elapsedMs || 0) / 1000).toFixed(1) }} 秒</span>
      <Button v-if="message.role === 'assistant' && !message.active && message.status === 'completed'" variant="outline" size="sm" class="message-action" :disabled="busy" @click="$emit('select')">使用此版本</Button>
    </div>
  </article>
</template>

<style scoped>
.ai-message{display:flex;flex-direction:column;align-items:flex-start;gap:8px;min-width:0;margin:18px 0;font-size:13px;overflow-wrap:anywhere}
.ai-message.user{align-items:flex-end;margin-left:28px}.ai-message.assistant{margin-right:12px}
.bubble{max-width:100%;min-width:0;width:100%;border-radius:10px;padding:12px;background:var(--app-bg-dialog)}
.user .bubble{width:fit-content;background:color-mix(in srgb,var(--color-primary) 13%,var(--app-bg-dialog));border:1px solid color-mix(in srgb,var(--color-primary) 20%,transparent)}
.user-content{white-space:pre-wrap}.alternative .bubble{border:1px dashed var(--app-border-light)}
.version{display:block;margin-bottom:8px;font-size:10px;color:var(--app-text-muted)}
.thinking{margin-bottom:10px;color:var(--app-text-muted);font-size:12px}.thinking summary{cursor:pointer;padding:3px 0}.thinking pre{white-space:pre-wrap;font:inherit;line-height:1.7;border-left:2px solid var(--app-border-light);padding:8px 12px;max-height:360px;overflow:auto}
.actions{display:flex;align-items:center;flex-wrap:wrap;gap:6px;max-width:100%}.user .actions{justify-content:flex-end}
.message-action{height:26px;padding:0 8px;font-size:11px;gap:4px}.message-action :deep(svg){width:12px;height:12px}
.usage{border-left:1px solid var(--app-border-light);padding-left:8px;margin-left:2px;color:var(--app-text-muted);font-size:10px;line-height:1.6}
.muted{font-size:10px;color:var(--app-text-muted);margin-top:6px}.message-error{font-size:11px;color:var(--color-destructive,#e16b6b);margin-top:7px}
</style>
