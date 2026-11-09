<script setup>
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { useSshStore } from '@/stores/ssh';
import { useAiStore, aiInvoke } from '@/stores/ai';
import { useSecurityStore } from '@/stores/security';
import { useCommandKnowledgeStore } from '@/stores/commandKnowledge';
import { findMatchedCommandInPayload } from '@/utils/sensitiveCommand';
import { Button } from '@/components/ui/button';
import { toast } from '@/composables/useToast';
const props = defineProps({ tools: { type: Array, default: () => [] }, requestId: String, messageId: String });
const ssh = useSshStore();
const ai = useAiStore(), security = useSecurityStore(), knowledge = useCommandKnowledgeStore();
const clock = ref(Date.now()), responding = ref(''); let timer;
const waiting = computed(() => props.tools.some(tool => tool.status === 'waiting_approval') && !!props.requestId);
watch(() => props.tools.find(tool => tool.status === 'waiting_approval')?.approvalId, () => { responding.value = ''; });
watch(waiting, value => {
  clearInterval(timer); clock.value = Date.now();
  if (value) { timer = setInterval(() => { clock.value = Date.now(); }, 1000); void knowledge.loadEntries().catch(() => {}); }
}, { immediate: true });
onBeforeUnmount(() => clearInterval(timer));
const names = { list_remote_directory: '列出目录', read_remote_text: '读取文本', stat_remote_path: '查看属性', execute_ssh_command: '执行 SSH 命令', execute_local_cmd: '执行本地 CMD 命令', run_registered_skill: '执行固定脚本' };
const states = { running: '查询中…', completed: '已完成', failed: '失败', stopped: '已停止', interrupted: '已中断', waiting_approval: '等待确认', approved: '已批准，尚未派发', denied: '已拒绝，未执行', expired: '确认过期，未执行', unknown: '远端状态未知' };
const skillFor = tool => tool.arguments?.skill;
const isCmd = tool => tool.name === 'execute_local_cmd' || skillFor(tool)?.targetKind === 'cmd';
const directoryActions = { current: '核实任务目录', change: '切换任务目录', list: '列出任务目录' };
const stepName = tool => (tool.name === 'terminal_directory' && directoryActions[tool.arguments?.action]) || names[tool.name] || tool.name;
const isExec = tool => tool.name === 'execute_ssh_command' || tool.name === 'run_registered_skill' || isCmd(tool);
const timeoutSecs = tool => skillFor(tool)?.timeoutSecs || 30;
const hasOutput = execution => !!(execution.stdout || execution.stderr);
function stepStatus(tool) {
  if (isExec(tool) && tool.status === 'running') return '执行中…';
  if (tool.execution?.endReason === 'timeout') return hasOutput(tool.execution) ? '超时，保留部分输出' : '超时，未收到输出';
  if (tool.status === 'unknown') return isCmd(tool) ? '本地命令结果未知' : '远端状态未知';
  if (tool.status === 'completed' && tool.execution && !tool.execution.outputComplete) return '命令已退出，输出不完整';
  return states[tool.status] || tool.status;
}
const remaining = tool => Math.max(0, Math.ceil(((tool.approvalExpiresAt || 0) - clock.value) / 1000));
const canDecide = tool => tool.status === 'waiting_approval' && !!tool.approvalId && remaining(tool) > 0 && props.requestId
  && ai.active?.requestId === props.requestId && !ai.active.cancelRequested && !security.isLocked && !responding.value;
const sensitive = tool => findMatchedCommandInPayload(tool.arguments?.command, knowledge.sensitiveRules)?.content;
async function decide(tool, approved) {
  if (!canDecide(tool)) return;
  responding.value = tool.approvalId;
  try { await aiInvoke('ai_respond_tool_approval', { requestId: props.requestId, messageId: props.messageId,
    callId: tool.callId, approvalId: tool.approvalId, approved }); }
  catch (e) { responding.value = ''; toast.error(String(e)); }
}
function resultText(text) {
  try { const result = JSON.parse(text); return typeof result.data?.text === 'string'
    ? `${result.data.path} · 字节 ${result.data.offset}–${result.data.nextOffset}${result.data.truncated ? '（内容未读完）' : ''}\n${result.data.text}`
    : JSON.stringify(result, null, 2); } catch { return text; }
}
</script>

<template>
  <div v-if="tools.length" class="tool-steps" aria-label="工具调用记录">
    <details v-for="(tool, index) in tools" :key="tool.callId" class="tool-step" :open="['running', 'waiting_approval', 'approved', 'unknown'].includes(tool.status)">
      <summary :title="tool.callId"><span :class="{ running: tool.status === 'running' }">{{ index + 1 }} · {{ stepName(tool) }}</span><span>{{ stepStatus(tool) }}</span></summary>
      <div class="tool-details">
        <p :title="tool.target">目标：{{ isCmd(tool) ? '本机 Windows CMD' : ssh.getSession(tool.target)?.name || tool.target }}</p>
        <template v-if="isExec(tool)">
          <p>{{ isCmd(tool) ? '执行目录' : '请求目录' }}：{{ tool.root }} · {{ skillFor(tool)?.interpreter || (isCmd(tool) ? 'cmd.exe' : '/bin/sh') }} · 非交互 · 最长 {{ timeoutSecs(tool) }} 秒</p>
          <template v-if="skillFor(tool)">
            <p :title="skillFor(tool).revision">Skill @{{ skillFor(tool).id }} · {{ skillFor(tool).version }} · {{ skillFor(tool).revision.slice(0, 12) }}</p>
            <p>固定入口：{{ skillFor(tool).script }}</p>
            <pre v-if="Object.keys(tool.arguments.parameters || {}).length" aria-label="脚本参数">{{ JSON.stringify(tool.arguments.parameters, null, 2) }}</pre>
          </template>
          <p>用途：{{ tool.arguments?.purpose }}</p>
          <pre class="command" :aria-label="skillFor(tool) ? '固定脚本与位置参数' : '待执行的完整命令'">{{ tool.arguments?.command }}</pre>
          <p v-if="sensitive(tool)" class="warning">命中敏感命令规则：{{ sensitive(tool) }}</p>
          <p v-if="tool.status === 'waiting_approval'" class="warning">命令可改变{{ isCmd(tool) ? '本机' : '服务器' }}，请审阅完整内容。{{ remaining(tool) }} 秒后确认失效，等待也计入本次请求总超时。</p>
          <div v-if="tool.status === 'waiting_approval'" class="approval-actions">
            <Button variant="outline" size="sm" :disabled="!canDecide(tool)" @click="decide(tool, false)">拒绝</Button>
            <Button size="sm" :disabled="!canDecide(tool)" @click="decide(tool, true)">确认执行</Button>
          </div>
          <template v-if="tool.execution">
            <p v-if="tool.execution.elapsedMs">执行耗时：{{ (tool.execution.elapsedMs / 1000).toFixed(1) }} 秒</p>
            <p v-if="tool.execution.exitCode !== null && tool.execution.exitCode !== undefined">退出码：{{ tool.execution.exitCode }}</p>
            <p v-if="tool.execution.signal">退出信号：{{ tool.execution.signal }}</p>
            <p v-if="tool.execution.truncated">输出已截断；最多保留 stdout/stderr 合计 256 KiB，模型收到有界预览。</p>
            <p v-if="tool.execution.encodingReplaced">部分输出无法按 UTF-8 解码，已替换显示。</p>
            <p v-if="tool.execution.endReason === 'timeout'">执行超过 {{ timeoutSecs(tool) }} 秒，已请求终止；请核实{{ isCmd(tool) ? '本机' : '远端' }}状态。</p>
            <p v-if="tool.execution.outcome === 'exited' && !tool.execution.outputComplete">已收到退出状态，但输出通道未完整结束。</p>
            <p v-if="tool.execution.stdout">stdout</p><pre v-if="tool.execution.stdout">{{ tool.execution.stdout }}</pre>
            <p v-if="tool.execution.stderr">stderr</p><pre v-if="tool.execution.stderr">{{ tool.execution.stderr }}</pre>
          </template>
        </template>
        <template v-else-if="tool.name === 'terminal_directory'">
          <p>任务目录：{{ tool.root }}</p>
          <p v-if="tool.arguments?.path">切换目标：{{ tool.arguments.path }}</p>
        </template>
        <template v-else>
          <p>授权范围：{{ tool.root }}</p>
          <p>路径：{{ tool.arguments?.path || '未提供' }}<template v-if="tool.arguments?.offset"> · 偏移 {{ tool.arguments.offset }}</template><template v-if="tool.arguments?.limit"> · 限额 {{ tool.arguments.limit }}</template></p>
        </template>
        <p>{{ new Date(tool.startedAt).toLocaleString() }} · {{ (tool.elapsedMs / 1000).toFixed(1) }} 秒</p>
        <pre v-if="tool.result">{{ resultText(tool.result) }}</pre>
      </div>
    </details>
  </div>
</template>

<style scoped>
.approval-actions{display:flex;gap:8px;justify-content:flex-end;margin:8px 0}.warning{color:var(--app-text);margin:6px 0}.command{border:1px solid var(--app-border-light);border-radius:6px;padding:8px;max-height:320px}
.tool-steps{display:flex;flex-direction:column;gap:6px;margin-bottom:10px;min-width:0}.tool-step{border:1px solid var(--app-border-light);border-radius:8px;font-size:11px;color:var(--app-text-muted)}summary{cursor:pointer;padding:7px 9px;overflow-wrap:anywhere}summary>span+span{float:right;margin-left:8px}.running{color:var(--color-primary)}.tool-details{padding:0 9px 9px;line-height:1.7;overflow-wrap:anywhere}pre{white-space:pre-wrap;max-height:240px;overflow:auto;margin-top:6px;font:inherit;color:var(--app-text)}
</style>
