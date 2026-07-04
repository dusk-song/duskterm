<script setup>
import Button from '@/components/ui/button/Button.vue';
import Input from '@/components/ui/input/Input.vue';
import { toast } from '@/composables/useToast';
import { Lock } from '@lucide/vue';
import { nextTick, onMounted, ref } from 'vue';
import { useSecurityStore } from '@/stores/security';

const securityStore = useSecurityStore();
const password = ref('');
const loading = ref(false);
const error = ref('');
const inputRef = ref(null);

async function handleUnlock() {
  if (!password.value || loading.value) return;

  loading.value = true;
  error.value = '';

  try {
    const success = await securityStore.unlock(password.value);
    if (success) {
      toast.success('已解锁');
      password.value = '';
    } else {
      error.value = '密码错误，请重试';
      toast.error('解锁失败');
      password.value = '';
      nextTick(() => {
        const inputEl = inputRef.value?.$el?.querySelector('input');
        if (inputEl) inputEl.focus();
      });
    }
  } catch (e) {
    console.error(e);
    error.value = '验证出错，请稍后再试';
  } finally {
    loading.value = false;
  }
}

onMounted(() => {
  const inputEl = inputRef.value?.$el?.querySelector('input');
  if (inputEl) inputEl.focus();
});
</script>

<template>
  <div class="lock-screen" v-if="securityStore.isLocked">
    <div class="lock-card">
      <div class="lock-icon-wrapper">
        <Lock class="lock-icon" />
      </div>
      <h2>应用已锁定</h2>
      <p class="subtitle">请输入密码以继续</p>

      <form @submit.prevent="handleUnlock" class="input-group">
        <Input ref="inputRef" v-model="password" type="password" placeholder="输入密码" size="sm" class="h-12 w-full"
          :disabled="loading" />
        <div v-if="error" class="error-msg">{{ error }}</div>
        <Button :disabled="loading" :loading="loading" type="submit" class="w-full h-12">
          {{ loading ? '验证中...' : '解锁' }}
        </Button>
      </form>
    </div>
  </div>
</template>

<style scoped>
.lock-screen {
  --lock-overlay-bg: var(--niri-backdrop-bg, rgba(0, 0, 0, 0.6));
  --lock-overlay-blur: var(--niri-backdrop-blur, 6px);

  position: fixed;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--lock-overlay-bg);
  backdrop-filter: blur(var(--lock-overlay-blur));
  -webkit-backdrop-filter: blur(var(--lock-overlay-blur));
  z-index: var(--z-critical-overlay);
  padding: 16px;
}

[data-performance="reduced"] .lock-screen {
  --lock-overlay-blur: 0px;
}

.lock-card {
  width: 100%;
  max-width: 380px;
  padding: 32px 24px 28px;
  background: var(--mac-bg-secondary, #ffffff);
  border: 1px solid var(--mac-border, rgba(0, 0, 0, 0.08));
  border-radius: 16px;

  text-align: center;
  animation: fadeUp 0.3s ease-out;
}

.lock-icon-wrapper {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 72px;
  height: 72px;
  margin: 0 auto 20px;
  border-radius: 50%;
  background: var(--mac-accent-bg, rgba(0, 122, 255, 0.12));
  color: var(--mac-accent, #007aff);
}

.lock-icon {
  width: 32px;
  height: 32px;
}

h2 {
  margin: 0 0 6px;
  font-size: 20px;
  font-weight: 600;
  color: var(--mac-text-primary, #1d1d1f);
  letter-spacing: -0.3px;
}

.subtitle {
  margin: 0 0 24px;
  font-size: 14px;
  color: var(--mac-text-secondary, #86868b);
  line-height: 1.4;
}

.input-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-bottom: 0;
}

.error-msg {
  font-size: 13px;
  color: hsl(var(--destructive, 0 72% 51%));
  text-align: left;
  padding-left: 2px;
  min-height: 20px;
  line-height: 1.4;
}

:deep(.btn) {
  font-weight: 500;
}

@keyframes fadeUp {
  from {
    opacity: 0;
    transform: translateY(16px) scale(0.98);
  }

  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}

@media (max-width: 480px) {
  .lock-card {
    padding: 24px 16px 20px;
  }

  .lock-icon-wrapper {
    width: 60px;
    height: 60px;
  }

  .lock-icon {
    width: 28px;
    height: 28px;
  }
}
</style>
