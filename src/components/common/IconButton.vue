<script setup>
import { computed } from 'vue'
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'

const props = defineProps({
  icon: {
    type: [Object, Function, String],
    required: true,
  },

  size: {
    type: String,
    default: 'md',
  },

  active: {
    type: Boolean,
    default: false,
  },

  disabled: {
    type: Boolean,
    default: false,
  },

  ariaLabel: {
    type: String,
    required: true,
  },

  action: {
    type: Function,
    default: null,
  },

  /**
   * 是否显示 Tooltip
   */
  tooltip: {
    type: Boolean,
    default: true,
  },

  /**
   * 鼠标悬停多久后显示，单位 ms
   */
  tooltipDelay: {
    type: Number,
    default: 350,
  },

  /**
   * 首选展示方向：
   * top | bottom | left | right
   */
  tooltipSide: {
    type: String,
    default: 'bottom',
  },

  /**
   * Tooltip 与按钮间距，单位 px
   */
  tooltipSideOffset: {
    type: Number,
    default: 8,
  },
})

const sizeMap = {
  sm: '24px',
  md: '32px',
  lg: '40px',
}

const computedSize = computed(() => {
  return sizeMap[props.size] || props.size
})

const handleClick = (event) => {
  if (props.disabled) return

  props.action?.(event)
}
</script>

<template>
  <TooltipProvider :delay-duration="tooltipDelay">
    <Tooltip :disabled="disabled || !tooltip">
      <TooltipTrigger as-child>
        <button
          class="icon-button"
          :class="{
            active,
            disabled,
          }"
          :style="{
            '--icon-btn-size': computedSize,
          }"
          :aria-label="ariaLabel"
          :disabled="disabled"
          type="button"
          @click="handleClick"
        >
          <span class="icon-wrapper">
            <component
              v-if="typeof icon === 'object' || typeof icon === 'function'"
              :is="icon"
              class="icon-svg"
            />

            <img
              v-else-if="typeof icon === 'string'"
              :src="icon"
              alt=""
              class="icon-img"
            />

            <slot v-else />
          </span>
        </button>
      </TooltipTrigger>

      <TooltipContent
        class="icon-button-tooltip"
        :side="tooltipSide"
        :side-offset="tooltipSideOffset"
        :avoid-collisions="true"
        :collision-padding="8"
      >
        {{ ariaLabel }}
      </TooltipContent>
    </Tooltip>
  </TooltipProvider>
</template>

<style src="../../assets/styles/icon-button.css"></style>
