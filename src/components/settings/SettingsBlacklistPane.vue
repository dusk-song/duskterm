<script setup>
import Button from '@/components/ui/button/Button.vue';
import Select from '@/components/ui/select/Select.vue';
import SelectContent from '@/components/ui/select/SelectContent.vue';
import SelectItem from '@/components/ui/select/SelectItem.vue';
import SelectTrigger from '@/components/ui/select/SelectTrigger.vue';
import SelectValue from '@/components/ui/select/SelectValue.vue';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { HelpCircle, Plus, Trash2, Undo2 } from '@lucide/vue';

defineProps({
  localBlacklist: {
    type: Array,
    required: true
  },
  restoreDefaults: {
    type: Function,
    required: true
  },
  removeRule: {
    type: Function,
    required: true
  },
  openAddRuleModal: {
    type: Function,
    required: true
  }
});
</script>

<template>
  <div class="settings-content">
    <div class="settings-section">
      <div class="settings-section-title-wrap">
        <div class="settings-section-title">敏感命令拦截</div>
        <Tooltip>
          <TooltipTrigger>
            <HelpCircle class="section-tip-icon" />
          </TooltipTrigger>
          <TooltipContent>
            匹配正则的命令将被拦截。轻微：提醒后允许执行。严重：默认阻断，需二次确认。
          </TooltipContent>
        </Tooltip>
      </div>

      <div class="bl-actions">
        <Button size="sm" @click="openAddRuleModal">
          <Plus :size="14" /> 新增
        </Button>
        <Button size="sm" variant="outline" @click="restoreDefaults">
          <Undo2 :size="14" /> 恢复默认
        </Button>
      </div>

      <div v-if="localBlacklist.length === 0" class="bl-empty">暂无规则</div>

      <div v-else class="bl-table">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead class="w-[40px]">#</TableHead>
              <TableHead>正则表达式</TableHead>
              <TableHead class="w-[120px]">风险级别</TableHead>
              <TableHead class="w-[80px]">操作</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow v-for="(rule, index) in localBlacklist" :key="index">
              <TableCell class="text-muted-foreground">{{ index + 1 }}</TableCell>
              <TableCell><code class="text-base">{{ rule.pattern }}</code></TableCell>
              <TableCell>
                <Select v-model="rule.severity">
                  <SelectTrigger size="sm" class="w-[100px]">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent position="popper" side="bottom" align="start" :side-offset="4" :collision-padding="16">
                    <SelectItem value="warning">轻微</SelectItem>
                    <SelectItem value="critical">严重</SelectItem>
                  </SelectContent>
                </Select>
              </TableCell>
              <TableCell>
                <Button variant="destructive" size="icon" @click="removeRule(index)" aria-label="删除">
                  <Trash2 :size="14" />
                </Button>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>
    </div>
  </div>
</template>

<style scoped>
@import './settingsPaneShared.css';

.bl-actions {
  @apply flex gap-1.5 mb-2;
}

.bl-table {
  @apply border border-[hsl(var(--border))] rounded-[var(--radius-md)] overflow-hidden;
}

.bl-header {
  @apply flex items-center gap-2 px-2.5 py-[5px] text-[11px] font-semibold text-[hsl(var(--muted-foreground))] bg-[hsl(var(--secondary))] border-b border-[hsl(var(--border))] select-none;
}

.bl-row {
  @apply flex items-center gap-2 px-2.5 py-1 text-xs border-b border-[hsl(var(--border)/0.5)] transition-colors duration-100;
}

.bl-row:last-child {
  @apply border-b-0;
}

.bl-row:hover {
  background: hsl(var(--accent) / 0.06);
}

.bl-col-index {
  @apply w-7 shrink-0 text-center text-[11px] text-[hsl(var(--muted-foreground))];
}

.bl-col-pattern {
  @apply flex-1 min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-[11px] text-[hsl(var(--foreground))];
  font-family: 'Consolas', 'Cascadia Mono', 'Courier New', monospace;
}

.bl-header .bl-col-pattern {
  font-family: inherit;
}

.bl-col-severity-sel {
  @apply w-[72px] shrink-0;
}

.bl-col-action {
  @apply w-8 shrink-0 flex justify-center;
}

.bl-empty {
  @apply py-4 text-[hsl(var(--muted-foreground))] text-center text-xs;
}
</style>
