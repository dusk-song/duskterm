<script setup>
import { computed } from 'vue';
import { Button } from '@/components/ui/button';
const props = defineProps({ task: Object, busy: Boolean, canVerify: Boolean, hasError: Boolean });
defineEmits(['verified']);
const note = computed(() => {
  if (props.canVerify) return '有变更的结果未知。请先在终端核实实际状态。';
  if (props.task.evidenceStatus === 'user_verified') return '相关操作已人工核实。';
  if (props.task.status === 'paused' && !props.hasError) return '本轮已暂停。可以在输入框补充信息或继续任务。';
  return '';
});
</script>

<template>
  <div v-if="note" class="task-note">
    <p role="status">{{ note }}</p>
    <Button v-if="canVerify" variant="outline" size="sm" :disabled="busy" @click="$emit('verified')">核实结果</Button>
  </div>
</template>

<style scoped>
.task-note{display:flex;align-items:center;flex-wrap:wrap;gap:6px 10px;margin-top:8px;font-size:12px;color:var(--app-text-muted)}
</style>
