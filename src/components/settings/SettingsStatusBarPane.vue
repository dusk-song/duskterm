<script setup>
import Slider from '@/components/ui/slider/Slider.vue';
import Switch from '@/components/ui/switch/Switch.vue';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { HelpCircle } from '@lucide/vue';

defineProps({
  lightbarSettings: {
    type: Object,
    required: true
  },
  monitorSettings: {
    type: Object,
    required: true
  }
});
</script>

<template>
  <div class="settings-content scrollable-y is-scroll">
    <div class="settings-section idea-panel">
      <div class="settings-section-title-wrap">
        <div class="settings-section-title">灯条</div>
        <Tooltip>
          <TooltipTrigger>
            <HelpCircle class="section-tip-icon" />
          </TooltipTrigger>
          <TooltipContent>
            灯条显示在顶部菜单栏，根据键盘输入频率动态变化，提供视觉反馈。
          </TooltipContent>
        </Tooltip>
      </div>
      <div class="setting-row">
        <div class="setting-label">灯条颜色（起）</div>
        <input class="color-input" type="color" v-model="lightbarSettings.colorStart" />
      </div>
      <div class="setting-row">
        <div class="setting-label">灯条颜色（止）</div>
        <input class="color-input" type="color" v-model="lightbarSettings.colorEnd" />
      </div>
      <div class="setting-row">
        <div class="setting-label">灯条速度</div>
        <Slider v-model="lightbarSettings.speed" :min="0.6" :max="2.4" :step="0.1" class="line-slider" />
        <span class="setting-value">{{ Number(lightbarSettings.speed).toFixed(1) }}x</span>
      </div>
      <div class="setting-row">
        <div class="setting-label">拖尾效果</div>
        <Switch v-model="lightbarSettings.enableTrail" />
      </div>
      <div class="setting-row">
        <div class="setting-label">峰值保持</div>
        <Switch v-model="lightbarSettings.enablePeakHold" />
      </div>
    </div>

    <div class="settings-section idea-panel">
      <div class="settings-section-title-wrap">
        <div class="settings-section-title">服务器监控</div>
        <Tooltip>
          <TooltipTrigger>
            <HelpCircle class="section-tip-icon" />
          </TooltipTrigger>
          <TooltipContent>
            实时监控远程服务器的 CPU、内存、磁盘、网卡等系统资源使用情况。
          </TooltipContent>
        </Tooltip>
      </div>
      <div class="setting-row">
        <div class="setting-label">显示服务器监控</div>
        <Switch v-model="monitorSettings.showMonitor" />
      </div>
      <div class="setting-row">
        <div class="setting-label">CPU</div>
        <Switch v-model="monitorSettings.showCpu" />
      </div>
      <div class="setting-row">
        <div class="setting-label">内存</div>
        <Switch v-model="monitorSettings.showMemory" />
      </div>
      <div class="setting-row">
        <div class="setting-label">硬盘</div>
        <Switch v-model="monitorSettings.showDisk" />
      </div>
      <div class="setting-row">
        <div class="setting-label">网卡速率</div>
        <Switch v-model="monitorSettings.showNet" />
      </div>
      <div class="setting-row">
        <div class="setting-label">刷新频率</div>
        <Slider v-model="monitorSettings.refreshIntervalMs" :min="500" :max="5000" :step="100" class="line-slider" />
        <span class="setting-value">{{ monitorSettings.refreshIntervalMs }} ms</span>
      </div>
      <div class="setting-row">
        <div class="setting-label">磁盘刷新</div>
        <Slider v-model="monitorSettings.diskIntervalMs" :min="2000" :max="20000" :step="500" class="line-slider" />
        <span class="setting-value">{{ monitorSettings.diskIntervalMs }} ms</span>
      </div>

      <div class="settings-section-title-wrap">
        <div class="settings-section-title">监控外观</div>
        <Tooltip>
          <TooltipTrigger>
            <HelpCircle class="section-tip-icon" />
          </TooltipTrigger>
          <TooltipContent>
            自定义监控数据的显示颜色，区分本机和远程服务器状态。
          </TooltipContent>
        </Tooltip>
      </div>
      <div class="setting-row">
        <div class="setting-label">本机指示色</div>
        <input class="color-input" type="color" v-model="monitorSettings.localColor" />
      </div>
      <div class="setting-row">
        <div class="setting-label">远程指示色</div>
        <input class="color-input" type="color" v-model="monitorSettings.remoteColor" />
      </div>
      <div class="setting-row">
        <div class="setting-label">标签颜色</div>
        <input class="color-input" type="color" v-model="monitorSettings.labelColor" />
      </div>
    </div>
  </div>
</template>

<style scoped>
@import './settingsPaneShared.css';
</style>
