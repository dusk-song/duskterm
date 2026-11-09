<script setup>
defineProps({ preview: { type: Object, required: true }, status: String });

const phases = { check: '检查', change: '拟变更', verify: '验证', recovery: '恢复预案' };
</script>

<template>
  <section class="deployment-plan" aria-label="SSH 部署计划">
    <div class="plan-heading">
      <strong>SSH 部署计划</strong>
      <span role="status">{{ preview.plan ? '草稿待审阅' : status === 'streaming' ? '起草中' : '起草未完成' }}</span>
    </div>
    <p :title="preview.target.id">目标：{{ preview.target.label }}</p>
    <template v-if="preview.plan">
      <div v-if="preview.plan.pendingInputs.length" class="pending-inputs">
        <strong>待核实</strong>
        <ul><li v-for="(item, index) in preview.plan.pendingInputs" :key="index">{{ item }}</li></ul>
      </div>
      <ol class="plan-steps">
        <li v-for="(step, index) in preview.plan.steps" :key="index">
          <details>
            <summary>{{ index + 1 }} · {{ phases[step.phase] }} · {{ step.title }}</summary>
            <p>{{ step.detail }}</p>
            <pre v-if="step.command" aria-label="拟议命令">{{ step.command }}</pre>
          </details>
        </li>
      </ol>
    </template>
  </section>
</template>

<style scoped>
.deployment-plan{border:1px solid var(--app-border-light);border-radius:8px;padding:10px;margin-bottom:10px;font-size:12px;line-height:1.6;overflow-wrap:anywhere}
.plan-heading{display:flex;justify-content:space-between;flex-wrap:wrap;gap:6px;margin-bottom:4px}
.plan-heading span,.pending-inputs{color:var(--app-text-muted)}
.pending-inputs{margin-top:8px}.pending-inputs ul{list-style:disc;padding-left:18px}
.plan-steps{display:grid;gap:6px;margin-top:8px}
details{border:1px solid var(--app-border-light);border-radius:6px;padding:6px 8px}
summary{cursor:pointer;font-weight:500}
details p,pre{margin-top:6px;white-space:pre-wrap}
pre{max-height:160px;overflow:auto;font:inherit;color:var(--app-text)}
</style>
