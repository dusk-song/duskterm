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

const props = defineProps({ message: Object, canRegenerate: Boolean, canVerify: Boolean, busy: Boolean, requestId: String });
const streaming = computed(() => props.message.status === 'streaming');
const isThinking = computed(() => streaming.value
  && !!props.message.reasoning && (props.message.phase ? props.message.phase === 'reasoning' : !props.message.content));
const thinkingChoice = ref(null);
const recordsChoice = ref(null);
const sawApproval = ref(false);
const thinkingOpen = computed(() => thinkingChoice.value ?? streaming.value);
const hasError = computed(() => ['stopped', 'interrupted', 'failed'].includes(props.message.status));
const hasRecords = computed(() => !!(props.message.tools?.length || props.message.task?.contextSnapshot));
const waitingApproval = computed(() => !!props.requestId && streaming.value
  && props.message.tools?.some(tool => tool.status === 'waiting_approval'));
const recordsOpen = computed(() => waitingApproval.value || (recordsChoice.value ?? (streaming.value && sawApproval.value)));
const showAnswer = computed(() => !!props.message.content && !(hasError.value && props.message.tools?.length));
const activity = computed(() => {
  if (props.message.deployment) return '正在起草部署计划…';
  if (props.message.task?.status === 'summarizing') return '正在整理已取得的结果…';
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
watch(() => props.message.id, () => {
  thinkingChoice.value = null;
  recordsChoice.value = null;
  sawApproval.value = waitingApproval.value;
  followReasoning.value = true;
});
watch(waitingApproval, waiting => { if (waiting) sawApproval.value = true; }, { immediate: true });
watch([() => props.message.reasoning, thinkingOpen], scrollReasoning, { flush: 'post' });
onBeforeUnmount(() => { if (reasoningFrame !== null) cancelAnimationFrame(reasoningFrame); });
defineEmits(['regenerate', 'select', 'verified']);
async function copy(text) {
  try { await navigator.clipboard.writeText(text); toast.success('已复制'); }
  catch { toast.error('复制失败，请手动选择文本'); }
}
</script>

<template>
  <article class="ai-message" :class="[message.role, { alternative: !message.active && message.role === 'assistant' }]">
    <div class="bubble">
      <AiDeploymentPlan v-if="message.deployment" :preview="message.deployment" :status="message.status" />
      <span v-if="message.role === 'assistant' && !message.active && message.status === 'completed'" class="version">历史版本</span>
      <details v-if="message.reasoning" class="thinking" :open="thinkingOpen">
        <summary @click.prevent="thinkingChoice = !thinkingOpen"><span v-if="isThinking" class="thinking-indicator" aria-hidden="true" />{{ isThinking ? '思考中…' : '思考过程' }}</summary>
        <pre v-if="thinkingOpen" ref="reasoningContent" @scroll="followReasoning = $event.target.scrollHeight - $event.target.scrollTop - $event.target.clientHeight < 24">{{ message.reasoning }}</pre>
      </details>
      <div v-if="message.role === 'user'" class="user-content">{{ message.content }}</div>
      <template v-else>
        <AiMarkdown v-if="showAnswer" :content="message.content" :streaming="message.status === 'streaming'" />
        <p v-if="hasError" class="message-error">{{ message.error || '本轮未生成完整结论。' }}</p>
        <p v-for="notice in message.terminalNotices" :key="notice" class="terminal-notice" role="status">{{ notice }}</p>
        <AiDiagnosticTask v-if="message.task && message.status !== 'streaming'" :task="message.task"
          :busy="busy" :can-verify="canVerify" :has-error="hasError" @verified="$emit('verified')" />
        <details v-if="hasRecords" class="execution-records" :open="recordsOpen">
          <summary @click.prevent="!waitingApproval && (recordsChoice = !recordsOpen)">执行记录<template v-if="message.tools?.length"> · {{ message.tools.length }} 步</template></summary>
          <div v-if="recordsOpen" class="record-body">
            <p v-if="message.task?.target">目标 {{ message.task.target.label || message.task.target.id }}</p>
            <p v-if="message.task?.directory">任务目录 {{ message.task.directory }}</p>
            <AiToolSteps v-if="message.tools?.length" :tools="message.tools" :request-id="requestId" :message-id="message.id" />
            <details v-if="message.task?.contextSnapshot">
              <summary>引用的历史记录</summary><pre>{{ message.task.contextSnapshot }}</pre>
            </details>
          </div>
        </details>
      </template>
      <p v-if="message.status === 'streaming' && !isThinking" role="status" class="muted">{{ activity }}</p>
    </div>
    <div class="actions">
      <IconButton v-if="showAnswer" :icon="Copy" size="sm" aria-label="复制" tooltip-side="top" :action="() => copy(message.content)" />
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
.terminal-notice{margin-top:8px;padding:6px 8px;border-left:2px solid var(--app-border-light);color:var(--app-text-muted);font-size:12px}
.execution-records{margin-top:10px;border-top:1px solid var(--app-border-light);font-size:11px;color:var(--app-text-muted)}
.execution-records summary{cursor:pointer;padding:6px 0}.record-body>p{margin-bottom:5px}
.record-body pre{font:inherit;white-space:pre-wrap;max-height:200px;overflow:auto}
.thinking{margin-bottom:10px;color:var(--app-text-muted);font-size:12px}.thinking summary{cursor:pointer;padding:3px 0}.thinking pre{white-space:pre-wrap;font:inherit;line-height:1.7;border-left:2px solid var(--app-border-light);padding:8px 12px;max-height:360px;overflow:auto}
.thinking-indicator{display:inline-block;width:6px;height:6px;margin-right:6px;border-radius:50%;background:var(--color-primary);vertical-align:middle}
.actions{display:flex;align-items:center;flex-wrap:wrap;gap:6px;max-width:100%}.user .actions{justify-content:flex-end}
.message-action{height:26px;padding:0 8px;font-size:11px;gap:4px}.message-action :deep(svg){width:12px;height:12px}
.usage{border-left:1px solid var(--app-border-light);padding-left:8px;margin-left:2px;color:var(--app-text-muted);font-size:10px;line-height:1.6}
.muted{font-size:10px;color:var(--app-text-muted);margin-top:6px}.message-error{font-size:11px;color:var(--color-destructive,#e16b6b);margin-top:7px}
</style>
