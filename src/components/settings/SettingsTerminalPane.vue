<script setup>
import Switch from '@/components/ui/switch/Switch.vue';

defineProps({
  terminalThemeSettings: {
    type: Object,
    required: true
  }
});
</script>

<template>
  <div class="settings-content scrollable-y is-scroll">
    <div class="settings-section idea-panel">
      <div class="settings-section-title-wrap">
        <div class="settings-section-title">显示</div>
      </div>
      <div class="setting-row">
        <span class="setting-label">行号显示</span>
        <Switch v-model="terminalThemeSettings.showLineNumbers" />
      </div>
    </div>
  </div>
</template>

<style scoped>
@import './settingsPaneShared.css';
</style>
