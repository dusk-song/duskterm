<script setup>
import Badge from '@/components/ui/badge/Badge.vue';
import Button from '@/components/ui/button/Button.vue';
import Input from '@/components/ui/input/Input.vue';
import Separator from '@/components/ui/separator/Separator.vue';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { HelpCircle, Lock, Shield } from '@lucide/vue';

const props = defineProps({
  securityStore: {
    type: Object,
    required: true
  },
  currentLockPassword: {
    type: String,
    default: ''
  },
  lockPassword: {
    type: String,
    default: ''
  },
  lockPasswordConfirm: {
    type: String,
    default: ''
  },
  setAppPassword: {
    type: Function,
    required: true
  },
  verifyAndChange: {
    type: Function,
    required: true
  }
});

const emit = defineEmits([
  'update:currentLockPassword',
  'update:lockPassword',
  'update:lockPasswordConfirm'
]);
</script>

<template>
  <div class="settings-content scrollable-y">
    <div class="security-card idea-panel">
      <div class="security-header">
        <div class="security-icon-wrap">
          <Shield class="security-main-icon" />
        </div>
        <div class="security-header-main">
          <div class="security-title-wrap">
            <h3>应用启动密码</h3>
            <Tooltip>
              <TooltipTrigger>
                <HelpCircle class="section-tip-icon" />
              </TooltipTrigger>
              <TooltipContent>
                设置密码后，每次启动应用都需要验证，避免他人直接访问会话与凭据。
              </TooltipContent>
            </Tooltip>
          </div>
        </div>
        <div class="security-status-wrap">
          <Badge v-if="securityStore.hasPassword" variant="success" class="security-state-tag">已启用</Badge>
          <Badge v-else variant="secondary" class="security-state-tag">未启用</Badge>
        </div>
      </div>

      <Separator class="security-divider" />

      <div class="security-tip">
        <Lock :size="14" />
        <span>建议使用 8 位以上密码，包含字母、数字与符号组合。</span>
      </div>

      <div class="password-form">
        <template v-if="securityStore.hasPassword">
          <div class="form-item">
            <label>当前密码</label>
            <Input type="password" size="sm" class="w-full" :model-value="currentLockPassword" placeholder="验证当前密码以修改"
              @update:model-value="emit('update:currentLockPassword', $event)" />
          </div>
        </template>

        <div class="form-item">
          <label>{{ securityStore.hasPassword ? '新密码' : '设置密码' }}</label>
          <Input type="password" size="sm" class="w-full" :model-value="lockPassword" placeholder="输入新密码 (留空则移除密码)"
            @update:model-value="emit('update:lockPassword', $event)" />
        </div>
        <div class="form-item">
          <label>确认密码</label>
          <Input type="password" size="sm" class="w-full" :model-value="lockPasswordConfirm" placeholder="再次输入确认"
            @update:model-value="emit('update:lockPasswordConfirm', $event)" />
        </div>

        <div class="form-actions">
          <Button v-if="securityStore.hasPassword" @click="verifyAndChange">更改 / 移除密码</Button>
          <Button v-else @click="setAppPassword">启用密码</Button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
@import './settingsPaneShared.css';

/* .security-card { 
  @apply border border-[var(--mac-border)] rounded-[10px] p-4 px-[18px];
  background: linear-gradient(180deg, rgba(255, 255, 255, 0.02) 0%, rgba(0, 0, 0, 0.06) 100%), var(--mac-bg-primary);
} */

.security-header {
  @apply flex items-center gap-3;
}

.security-icon-wrap {
  @apply w-[34px] h-[34px] rounded-lg flex items-center justify-center shrink-0;
  border: 1px solid rgba(24, 144, 255, 0.45);
  background: rgba(24, 144, 255, 0.12);
}

.security-main-icon {
  @apply text-lg text-[var(--color-info)];
}

.security-header-main {
  @apply flex-1 min-w-0;
}

.security-title-wrap {
  @apply flex items-center gap-2;
}

.security-header-main h3 {
  @apply m-0 text-base leading-[1.4] text-[var(--mac-text-primary)];
}

.security-header-main p {
  @apply mt-1.5 text-[var(--mac-text-secondary)] leading-[1.6] text-[13px];
}

.security-status-wrap {
  @apply shrink-0;
}

.security-state-tag {
  @apply mr-0 px-2 py-0.5 rounded-sm font-semibold;
}

.security-divider {
  @apply my-4;
}

.security-tip {
  @apply flex items-center gap-2 text-[var(--color-warning)] rounded-lg px-2.5 py-2 text-xs;
  border: 1px solid rgba(var(--warning, 250 173 20), 0.35);
  background: rgba(var(--warning, 250 173 20), 0.08);
}

.password-form {
  @apply mt-3.5 flex flex-col gap-3;
}

.form-item {
  @apply flex items-start gap-2;
}

.form-item label {
  @apply w-[100px] shrink-0 min-h-8 pt-1.5 text-right pr-3 text-[13px] font-normal text-[var(--app-text-muted)];
}

.form-actions {
  @apply mt-0.5 flex justify-end pl-[100px];
}

@media (max-width: 768px) {
  .security-header {
    @apply flex-wrap;
  }

  .security-status-wrap {
    @apply w-full;
  }

  .form-actions {
    justify-content: stretch;
  }
}
</style>
