<script setup>
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { Copy, RotateCcw } from '@lucide/vue';
import { Button } from '@/components/ui/button';
import IconButton from '@/components/common/IconButton.vue';
import { formatTokens } from '@/utils/ai';
import { toast } from '@/composables/useToast';
import AiMarkdown from './AiMarkdown.vue';
import AiToolSteps from './AiToolSteps.vue';
import AiDiagnosticTask from './AiDiagnosticTask.vue';
import AiDeploymentPlan from './AiDeploymentPlan.vue';

const props = defineProps({ message: Object, canRegenerate: Boolean, busy: Boolean, requestId: String });
// The first body chunk ends the thinking phase; streaming continues for the answer.
const isThinking = computed(() => props.message.status === 'streaming'
  && !!props.message.reasoning && (props.message.phase ? props.message.phase === 'reasoning' : !props.message.content));
const thinkingOpen = ref(false);
const activity = computed(() => {
  if (props.message.deployment) return '正在起草部署计划…';
  const tools=props.message.tools || [];
  if (tools.some(tool=>tool.status==='waiting_approval')) return '等待你确认命令…';
  if (tools.some(tool=>['execute_ssh_command','execute_local_cmd'].includes(tool.name) && ['approved','running'].includes(tool.status))) return '正在处理命令…';
  return '正在生成…';
});
const reasoningContent = ref(null);
const followReasoning = ref(true);
let reasoningFrame = null;
function scrollReasoning() {
  if (!isThinking.value || !thinkingOpen.value || !followReasoning.value || reasoningFrame !== null) return;
  reasoningFrame = requestAnimationFrame(() => {
    reasoningFrame = null;
    if (isThinking.value && thinkingOpen.value && followReasoning.value && reasoningContent.value) {
      reasoningContent.value.scrollTop = reasoningContent.value.scrollHeight;
    }
  });
}
watch([() => props.message.id, isThinking], ([, thinking]) => {
  thinkingOpen.value = thinking;
  followReasoning.value = true;
}, { immediate: true });
watch([() => props.message.reasoning, thinkingOpen], scrollReasoning, { flush: 'post' });
onBeforeUnmount(() => { if (reasoningFrame !== null) cancelAnimationFrame(reasoningFrame); });
defineEmits(['regenerate', 'select', 'analyze', 'continue', 'verified']);
async function copy(text) {
  try { await navigator.clipboard.writeText(text); toast.success('已复制'); }
  catch { toast.error('复制失败，请手动选择文本'); }
}
</script>

<template>
  <article class="ai-message" :class="[message.role, { alternative: !message.active && message.role === 'assistant' }]">
    <div class="bubble">
      <AiDiagnosticTask v-if="message.task && (!message.task.automatic || message.tools?.length || message.task.runNumber > 1)" :task="message.task" :tools="message.tools" :model-label="message.actualModelId || message.modelId" :service-name="message.serviceName" :busy="busy" @analyze="$emit('analyze')" @continue="$emit('continue')" @verified="$emit('verified')" />
      <AiDeploymentPlan v-if="message.deployment" :preview="message.deployment" :status="message.status" />
      <span v-if="message.role === 'assistant' && !message.active && message.status === 'completed'" class="version">历史版本</span>
      <details v-if="message.reasoning" class="thinking" :open="thinkingOpen">
        <summary @click.prevent="thinkingOpen = !thinkingOpen"><span v-if="isThinking" class="thinking-indicator" aria-hidden="true" />{{ isThinking ? '思考中…' : '思考过程' }}</summary>
        <pre ref="reasoningContent" @scroll="followReasoning = $event.target.scrollHeight - $event.target.scrollTop - $event.target.clientHeight < 24">{{ message.reasoning }}</pre>
      </details>
      <div v-if="message.role === 'user'" class="user-content">{{ message.content }}</div>
      <template v-else>
        <AiToolSteps :tools="message.tools || []" :request-id="requestId" :message-id="message.id" />
        <AiMarkdown :content="message.content" :streaming="message.status === 'streaming'" />
      </template>
      <p v-if="message.status === 'streaming' && !isThinking" role="status" class="muted">{{ activity }}</p>
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
.thinking-indicator{display:inline-block;width:6px;height:6px;margin-right:6px;border-radius:50%;background:var(--color-primary);vertical-align:middle;animation:thinking-pulse 1.2s ease-in-out infinite}
@keyframes thinking-pulse{0%,100%{opacity:.35}50%{opacity:1}}
@media(prefers-reduced-motion:reduce){.thinking-indicator{animation:none}}
.actions{display:flex;align-items:center;flex-wrap:wrap;gap:6px;max-width:100%}.user .actions{justify-content:flex-end}
.message-action{height:26px;padding:0 8px;font-size:11px;gap:4px}.message-action :deep(svg){width:12px;height:12px}
.usage{border-left:1px solid var(--app-border-light);padding-left:8px;margin-left:2px;color:var(--app-text-muted);font-size:10px;line-height:1.6}
.muted{font-size:10px;color:var(--app-text-muted);margin-top:6px}.message-error{font-size:11px;color:var(--color-destructive,#e16b6b);margin-top:7px}
</style>
