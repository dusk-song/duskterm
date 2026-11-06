<script setup>
import { computed } from 'vue';
import { Button } from '@/components/ui/button';
const props = defineProps({ task: Object, tools: Array, busy: Boolean, modelLabel: String, serviceName: String });
defineEmits(['analyze']);
const states = { running:'诊断中', waiting_confirmation:'等待确认', completed:'本轮诊断结束', failed:'诊断未完成', stopped:'诊断已停止', interrupted:'上次诊断已中断', budget_exhausted:'预算已用尽', timed_out:'诊断超时' };
const dispatched = computed(() => (props.tools || []).filter(tool => tool.execution && !['not_started','rejected'].includes(tool.execution.outcome)).length);
const pending = computed(() => ['running','waiting_confirmation'].includes(props.task.status));
</script>

<template>
  <section class="diagnostic-task" aria-label="诊断任务">
    <div class="task-heading"><strong>{{ task.analysisOnly ? '仅分析已有证据' : 'SSH 诊断任务' }}</strong><span role="status">{{ states[task.status] || task.status }}</span></div>
    <p class="goal">{{ task.goal }}</p>
    <p>绑定目标：{{ task.target.label || task.target.id }}</p>
    <p v-if="modelLabel">本任务模型：{{ serviceName }} · {{ modelLabel }}</p>
    <p>模型 {{ task.modelCalls }}/{{ task.modelLimit }} 轮 · 工具 {{ task.toolCalls }}/{{ task.toolLimit }} 步<template v-if="!task.analysisOnly"> · 已派发 {{ dispatched }}/{{ task.commandLimit }} 条命令</template></p>
    <p v-if="pending && !task.analysisOnly">任务上限 5 分钟；工具按实际结果推进，命令逐条确认。</p>
    <p v-if="task.evidenceStatus === 'unknown_remote_state'" class="notice">存在远端状态未知的步骤，需先核实；继续分析不会重新执行命令。</p>
    <p v-else-if="!pending && task.evidenceStatus === 'insufficient_evidence'" class="notice">本轮实测证据不足，需要补充信息或批准检查。</p>
    <p v-if="!pending" class="notice">本轮结束不代表问题已修复；未验证服务恢复。</p>
    <details v-if="task.contextSnapshot"><summary>本次分析使用的历史证据预览</summary><pre>{{ task.contextSnapshot }}</pre></details>
    <Button v-if="!pending" variant="outline" size="sm" :disabled="busy" @click="$emit('analyze')">仅继续分析已有证据</Button>
  </section>
</template>

<style scoped>
.diagnostic-task{border:1px solid var(--app-border-light);border-radius:8px;padding:10px;margin-bottom:10px;min-width:0;font-size:11px;line-height:1.7;overflow-wrap:anywhere}.task-heading{display:flex;justify-content:space-between;flex-wrap:wrap;gap:6px;margin-bottom:5px}.task-heading strong{color:var(--app-text);font-size:12px}.goal{white-space:pre-wrap;color:var(--app-text);max-height:100px;overflow:auto}.notice{color:var(--app-text-muted);margin:5px 0}details{margin:8px 0}summary{cursor:pointer}pre{font:inherit;white-space:pre-wrap;max-height:200px;overflow:auto}button{margin-top:7px}
</style>
