<script setup>
import { computed } from 'vue';
import { Button } from '@/components/ui/button';
const props = defineProps({ task: Object, tools: Array, busy: Boolean, modelLabel: String, serviceName: String });
defineEmits(['analyze', 'continue']);
const states = { running:'诊断中', waiting_confirmation:'等待确认', summarizing:'正在整理已有证据', completed:'本轮运行结束', paused:'本轮已暂停，可继续检查', summary_failed:'证据整理未完成', failed:'诊断未完成', stopped:'诊断已停止', interrupted:'上次诊断已中断', budget_exhausted:'本轮已达到运行上限', timed_out:'本轮诊断超时' };
const dispatched = computed(() => (props.tools || []).filter(tool => tool.execution && !['not_started','rejected'].includes(tool.execution.outcome)).length);
const pending = computed(() => ['running','waiting_confirmation','summarizing'].includes(props.task.status));
const localCmd = computed(() => props.task.targetKind === 'cmd');
const heading = computed(() => {
  if (props.task.analysisOnly) return '仅分析已有证据';
  if (props.task.skill) return `固定脚本 @${props.task.skill.id}`;
  const terminal = localCmd.value ? '本地 CMD' : 'SSH';
  return `${terminal} ${props.task.terminalTask ? '终端任务' : '诊断任务'}`;
});
const taskStates = { running:'执行任务中', paused:'本轮已暂停，可继续任务', failed:'任务未完成', stopped:'任务已停止', interrupted:'上次任务已中断', timed_out:'本轮任务超时' };
</script>

<template>
  <section class="diagnostic-task" :aria-label="task.terminalTask ? '终端任务' : '诊断任务'">
    <div class="task-heading"><strong>{{ heading }}</strong><span role="status">{{ (task.terminalTask && taskStates[task.status]) || states[task.status] || task.status }}</span></div>
    <p class="goal">{{ task.goal }}</p>
    <p>绑定目标：{{ task.target.label || task.target.id }}</p>
    <p v-if="task.terminalTask">任务目录：{{ task.directory || '待核实' }}</p>
    <p v-if="task.skill" :title="task.skill.revision">脚本版本：{{ task.skill.version }} · {{ task.skill.script }}</p>
    <p v-if="modelLabel">本轮模型：{{ serviceName }} · {{ modelLabel }}</p>
    <p><template v-if="!task.analysisOnly">第 {{ task.runNumber || 1 }} 轮{{ task.terminalTask ? '运行' : '检查' }} · </template>模型 {{ task.modelCalls }}/{{ task.modelLimit }} 次 · 工具 {{ task.toolCalls }}/{{ task.toolLimit }} 步<template v-if="!task.analysisOnly"> · 已派发 {{ dispatched }} 条命令</template></p>
    <p v-if="pending && !task.analysisOnly">{{ task.skill ? '仅运行一次选定脚本，执行前需确认。' : '单次运行最多 5 分钟，命令逐条确认。' }}</p>
    <p v-if="task.evidenceStatus === 'unknown_remote_state'" class="notice">存在{{ localCmd ? '本机' : '远端' }}状态未知的步骤，需先核实；继续分析不会重新执行命令。</p>
    <p v-else-if="!pending && task.evidenceStatus === 'insufficient_evidence'" class="notice">本轮实测证据不足，需要补充信息或批准检查。</p>
    <p v-if="!pending && !task.terminalTask" class="notice">本轮结束不代表问题已修复；未验证服务恢复。</p>
    <details v-if="task.contextSnapshot"><summary>本次分析使用的历史证据预览</summary><pre>{{ task.contextSnapshot }}</pre></details>
    <div v-if="!pending" class="task-actions">
      <Button v-if="!task.analysisOnly && !task.skill" variant="outline" size="sm" :disabled="busy" @click="$emit('continue')">{{ task.terminalTask ? '继续任务' : '继续检查' }}</Button>
      <Button variant="outline" size="sm" :disabled="busy" @click="$emit('analyze')">仅继续分析已有证据</Button>
    </div>
  </section>
</template>

<style scoped>
.task-actions{display:flex;flex-wrap:wrap;gap:6px}
.diagnostic-task{border:1px solid var(--app-border-light);border-radius:8px;padding:10px;margin-bottom:10px;min-width:0;font-size:11px;line-height:1.7;overflow-wrap:anywhere}.task-heading{display:flex;justify-content:space-between;flex-wrap:wrap;gap:6px;margin-bottom:5px}.task-heading strong{color:var(--app-text);font-size:12px}.goal{white-space:pre-wrap;color:var(--app-text);max-height:100px;overflow:auto}.notice{color:var(--app-text-muted);margin:5px 0}details{margin:8px 0}summary{cursor:pointer}pre{font:inherit;white-space:pre-wrap;max-height:200px;overflow:auto}button{margin-top:7px}
</style>
