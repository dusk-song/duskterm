<script setup>
import Button from '@/components/ui/button/Button.vue';
import Switch from '@/components/ui/switch/Switch.vue';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { useTheme } from '@/composables/useTheme';
import { getTerminalTheme, getTerminalThemeOptions } from '@/utils/terminalTheme';
import { computed } from 'vue';

const props = defineProps({
  terminalThemeSettings: {
    type: Object,
    required: true
  },
  commandHistorySettings: {
    type: Object,
    required: true
  },
  clearCommandHistory: {
    type: Function,
    required: true
  }
});

const emit = defineEmits(['preview-change']);
const { isDark } = useTheme();
const terminalThemeOptions = getTerminalThemeOptions();
const selectedTheme = computed(() => getTerminalTheme(props.terminalThemeSettings.theme, isDark.value));
const themeSwatches = computed(() => [
  selectedTheme.value.black,
  selectedTheme.value.red,
  selectedTheme.value.green,
  selectedTheme.value.yellow,
  selectedTheme.value.blue,
  selectedTheme.value.magenta,
  selectedTheme.value.cyan,
  selectedTheme.value.foreground,
].filter(Boolean));

const updateTerminalTheme = (theme) => {
  props.terminalThemeSettings.theme = theme;
  emit('preview-change', { ...props.terminalThemeSettings });
};
</script>

<template>
  <div class="settings-content scrollable-y is-scroll">
    <div class="settings-section idea-panel">
      <div class="settings-section-title-wrap">
        <div class="settings-section-title">显示</div>
      </div>
      <div class="setting-row">
        <span class="setting-label">终端配色</span>
        <Select :model-value="terminalThemeSettings.theme" @update:model-value="updateTerminalTheme">
          <SelectTrigger size="sm" class="terminal-theme-select" aria-label="终端配色">
            <SelectValue placeholder="选择终端配色" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem v-for="option in terminalThemeOptions" :key="option.key" :value="option.key">
              {{ option.name }}
            </SelectItem>
          </SelectContent>
        </Select>
        <span class="terminal-theme-swatches" aria-hidden="true">
          <i v-for="(color, index) in themeSwatches" :key="`${color}-${index}`" :style="{ backgroundColor: color }" />
        </span>
      </div>
      <div class="setting-row">
        <span class="setting-label">行号显示</span>
        <Switch v-model="terminalThemeSettings.showLineNumbers" />
      </div>
    </div>
    <div class="settings-section idea-panel">
      <div class="settings-section-title-wrap">
        <div class="settings-section-title">命令历史</div>
      </div>
      <div class="setting-row">
        <span class="setting-label">自动记录</span>
        <Switch v-model="commandHistorySettings.enabled" />
      </div>
      <div class="setting-row">
        <span class="setting-label">历史数据</span>
        <Button type="button" variant="outline" size="sm" @click="clearCommandHistory">清空历史</Button>
      </div>
    </div>
  </div>
</template>

<style scoped>
@import './settingsPaneShared.css';

.terminal-theme-select {
  width: 220px;
  flex: 0 1 220px;
}

.terminal-theme-swatches {
  display: inline-flex;
  flex: 0 0 auto;
  overflow: hidden;
  border: 1px solid var(--app-border-shadow);
  border-radius: 5px;
  background: var(--app-input-bg);
}

.terminal-theme-swatches i {
  width: 10px;
  height: 20px;
}

@media (max-width: 640px) {
  .terminal-theme-swatches {
    display: none;
  }
}
</style>
