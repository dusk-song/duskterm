<script setup>
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { HelpCircle } from '@lucide/vue';
import QuickCommands from '../misc/QuickCommands.vue';
</script>

<template>
  <div class="settings-content">
    <div class="settings-section idea-panel">
      <div class="settings-section-title-wrap">
        <div class="settings-section-title">命令中心</div>
        <Tooltip>
          <TooltipTrigger>
            <HelpCircle class="section-tip-icon" />
          </TooltipTrigger>
          <TooltipContent>
            维护常用命令模板，并在终端输入时触发快速提示。
          </TooltipContent>
        </Tooltip>
      </div>
      <div class="quick-commands-wrap">
        <QuickCommands :embedded="true" />
      </div>
    </div>
  </div>
</template>

<style scoped>
@import './settingsPaneShared.css';

.quick-commands-wrap {
  @apply rounded-none overflow-hidden border-0 bg-transparent;
}
</style>
