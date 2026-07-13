<script setup>
import Slider from '@/components/ui/slider/Slider.vue';
import Switch from '@/components/ui/switch/Switch.vue';

defineProps({ monitorSettings: { type: Object, required: true } });
</script>

<template>
  <div class="settings-content scrollable-y is-scroll">
    <div class="settings-section idea-panel">
      <div class="settings-section-title-wrap"><div class="settings-section-title">顶部监控</div></div>
      <div class="setting-row"><div class="setting-label">显示资源监控</div><Switch v-model="monitorSettings.showMonitor" /></div>
      <div class="setting-row"><div class="setting-label">CPU</div><Switch v-model="monitorSettings.showCpu" /></div>
      <div class="setting-row"><div class="setting-label">内存</div><Switch v-model="monitorSettings.showMemory" /></div>
      <div class="setting-row"><div class="setting-label">硬盘</div><Switch v-model="monitorSettings.showDisk" /></div>
      <div class="setting-row"><div class="setting-label">网络上下行</div><Switch v-model="monitorSettings.showNet" /></div>
      <div class="setting-row"><div class="setting-label">刷新频率</div><Slider v-model="monitorSettings.refreshIntervalMs" :min="500" :max="5000" :step="100" class="line-slider" /><span class="setting-value">{{ monitorSettings.refreshIntervalMs }} ms</span></div>
      <div class="setting-row"><div class="setting-label">磁盘刷新</div><Slider v-model="monitorSettings.diskIntervalMs" :min="2000" :max="20000" :step="500" class="line-slider" /><span class="setting-value">{{ monitorSettings.diskIntervalMs }} ms</span></div>
      <div class="settings-section-title-wrap"><div class="settings-section-title">监控外观</div></div>
      <div class="setting-row"><div class="setting-label">本机指示色</div><input class="color-input" type="color" v-model="monitorSettings.localColor" /></div>
      <div class="setting-row"><div class="setting-label">远程指示色</div><input class="color-input" type="color" v-model="monitorSettings.remoteColor" /></div>
      <div class="setting-row"><div class="setting-label">标签颜色</div><input class="color-input" type="color" v-model="monitorSettings.labelColor" /></div>
    </div>
  </div>
</template>

<style scoped>
@import './settingsPaneShared.css';
</style>
