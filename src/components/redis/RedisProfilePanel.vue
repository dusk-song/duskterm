<script setup>
import { computed, reactive, ref, watch } from 'vue';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import { Textarea } from '@/components/ui/textarea';
import SettingsSelect from '@/components/settings/SettingsSelect.vue';
import { toast } from '@/composables/useToast';
import { redisModeLabels } from '@/stores/redis';
import { invokeCommand } from '@/utils/ipc';
import { redisErrorMessage } from './redisRecovery';

const props = defineProps({
  profile: { type: Object, default: null },
  embedded: Boolean,
});
const emit = defineEmits(['cancel', 'saved']);
const formRef = ref(null);
const initial = props.profile;
const initialMode = initial?.redis?.mode ?? 'standalone';
const modes = Object.entries(redisModeLabels).map(([value, label]) => ({ value, label }));
const endpointLabel = ({ host, port }) => (host.includes(':') && !host.startsWith('[') ? '[' + host + ']' : host) + ':' + port;
const defaultPort = mode => mode === 'sentinel' ? 26379 : 6379;
const form = reactive({
  id: initial?.id || crypto.randomUUID(),
  name: initial?.name || '',
  group: initial?.group || '',
  host: initial?.host || '',
  port: initial?.port ?? 6379,
  mode: initialMode,
  seeds: (initial?.redis?.seeds ?? []).map(endpointLabel).join('\n'),
  username: initial?.username || '',
  db: initial?.redis?.db ?? 0,
  keySeparator: initial?.redis?.keySeparator ?? ':',
  tls: initial?.redis?.tls ?? false,
  sentinelService: initial?.redis?.sentinelService ?? '',
  sentinelUsername: initial?.redis?.sentinelUsername ?? '',
  sentinelTls: initial?.redis?.sentinelTls ?? false,
  connectTimeout: initial?.connect_timeout ?? 5,
  password: '',
  forgetPassword: false,
  sentinelPassword: '',
  forgetSentinelPassword: false,
});
const pending = ref('');
const sentinel = computed(() => form.mode === 'sentinel');
const cluster = computed(() => form.mode === 'cluster');
const endpointMatches = computed(() => {
  if (!initial || initialMode !== form.mode || initial.host !== form.host.trim() || initial.port !== Number(form.port)
    || initial.username !== form.username.trim() || Boolean(initial.redis?.tls) !== form.tls) return false;
  if (!sentinel.value && !cluster.value) return true;
  if (sentinel.value && (initial.redis.sentinelService !== form.sentinelService.trim()
    || initial.redis.sentinelUsername !== form.sentinelUsername.trim() || !!initial.redis.sentinelTls !== form.sentinelTls)) return false;
  try {
    return JSON.stringify(parseSeeds()) === JSON.stringify(initial.redis.seeds ?? []);
  } catch {
    return false;
  }
});
const retainedPassword = computed(() => endpointMatches.value && initial.redis?.passwordSaved && !form.forgetPassword);
const retainedSentinelPassword = computed(() => endpointMatches.value && initial.redis?.sentinelPasswordSaved && !form.forgetSentinelPassword);

watch(() => form.mode, (mode, previous) => {
  if (Number(form.port) === defaultPort(previous)) form.port = defaultPort(mode);
  form.password = '';
  form.sentinelPassword = '';
  form.forgetPassword = false;
  form.forgetSentinelPassword = false;
});

function parseSeeds() {
  const addresses = form.seeds.split(/[\n,]/).map(address => address.trim()).filter(Boolean);
  if (addresses.length > 15) throw new Error('最多填写 15 个备用节点。');
  return addresses.map(address => {
    const parsed = address.match(/^(\[[^\]\s]+\]|[^:\s]+):(\d+)$/);
    const port = Number(parsed?.[2]);
    if (!parsed || port < 1 || port > 65535) throw new Error('备用节点请使用 host:port 或 [IPv6]:port。');
    return { host: parsed[1], port };
  });
}

function readInteger(value, label, min, max) {
  const text = String(value ?? '').trim();
  const number = Number(text);
  if (!/^\d+$/.test(text) || !Number.isSafeInteger(number) || number < min || number > max) {
    throw new Error(`${label}须为 ${min}–${max} 的整数。`);
  }
  return number;
}

function connectionConfig() {
  if (!form.name.trim() || !form.host.trim()) throw new Error('请填写会话名称和主机地址。');
  const port = readInteger(form.port, '端口', 1, 65535);
  const db = cluster.value ? 0 : readInteger(form.db, '默认 DB', 0, 4294967295);
  const timeout = readInteger(form.connectTimeout, '连接超时', 1, 30);
  if (sentinel.value && !form.sentinelService.trim()) throw new Error('请填写哨兵主节点服务名。');
  if (new TextEncoder().encode(form.keySeparator).length > 32 || /[\x00-\x1f\x7f]/.test(form.keySeparator)) {
    throw new Error('键分隔符不能包含控制字符或超过 32 字节。');
  }
  const redis = { mode: form.mode, db, tls: form.tls, keySeparator: form.keySeparator };
  if (sentinel.value || cluster.value) redis.seeds = parseSeeds();
  if (sentinel.value) Object.assign(redis, {
    sentinelService: form.sentinelService.trim(),
    sentinelUsername: form.sentinelUsername.trim(),
    sentinelTls: form.sentinelTls,
  });
  return {
    id: form.id,
    name: form.name.trim(),
    group: form.group.trim(),
    host: form.host.trim(),
    port,
    username: form.username.trim(),
    auth_type: 'password',
    protocol: 'redis',
    remarks: initial?.remarks,
    last_connected: initial?.last_connected,
    connect_timeout: timeout,
    redis,
  };
}

async function submit(action) {
  if (pending.value) return;
  pending.value = action;
  const key = `redis-profile:${form.id}`;
  const testing = action === 'test';
  try {
    const config = connectionConfig();
    toast.loading({ content: testing ? '正在测试 Redis 连接…' : '正在保存 Redis 会话…', key, duration: 0 });
    await invokeCommand(testing ? 'redis_test_profile' : 'redis_save_profile', {
      config, password: form.password || null, forgetPassword: form.forgetPassword,
      sentinelPassword: sentinel.value ? form.sentinelPassword || null : null,
      forgetSentinelPassword: sentinel.value && form.forgetSentinelPassword,
    });
    toast.success({ content: testing ? 'Redis 连接测试成功' : 'Redis 会话已保存', key });
    if (testing) return;
    form.password = '';
    form.sentinelPassword = '';
    emit('saved', initial ? null : config);
  } catch (cause) {
    toast.error({ content: redisErrorMessage(cause), key, duration: 4000 });
  } finally {
    pending.value = '';
  }
}

defineExpose({
  pending,
  testConnection: () => submit('test'),
  save: () => formRef.value?.requestSubmit(),
});
</script>

<template>
  <section aria-label="Redis 会话配置" class="text-foreground" :class="{ 'h-full min-h-0 overflow-auto': !embedded }">
    <header v-if="!embedded" class="border-b border-border px-4 py-3">
      <p class="text-xs text-muted-foreground">会话 · Redis</p>
      <h2 class="mt-1 text-base font-semibold">{{ initial ? '编辑 Redis 会话' : '新建 Redis 会话' }}</h2>
    </header>
    <form ref="formRef" class="redis-profile-form max-w-xl" :class="{ 'px-4 py-3': !embedded }"
      autocomplete="off" novalidate @submit.prevent="submit('save')">
      <div class="form-item">
        <Label for="redis-profile-name">会话名称</Label>
        <Input id="redis-profile-name" v-model="form.name" class="redis-field-control" size="sm" required />
      </div>
      <div class="form-item">
        <Label for="redis-profile-group">分组标签</Label>
        <Input id="redis-profile-group" v-model="form.group" class="redis-field-control" size="sm" />
      </div>
      <div class="form-item">
        <Label for="redis-profile-mode">连接模式</Label>
        <div class="redis-field-control">
          <SettingsSelect item-class="text-xs" id="redis-profile-mode" v-model="form.mode" class="min-h-8 text-xs"
            :options="modes" :disabled="!!pending" />
        </div>
      </div>
      <div class="form-item">
        <Label for="redis-profile-host">{{ sentinel ? '哨兵地址' : cluster ? '集群入口地址' : '主机地址' }}</Label>
        <Input id="redis-profile-host" v-model="form.host" class="redis-field-control" size="sm" placeholder="127.0.0.1" required />
      </div>
      <div class="form-item">
        <Label for="redis-profile-port">端口</Label>
        <Input id="redis-profile-port" v-model="form.port" class="redis-field-control" type="text" inputmode="numeric"
          size="sm" :placeholder="String(defaultPort(form.mode))" required />
      </div>
      <div v-if="sentinel || cluster" class="form-item">
        <Label for="redis-profile-seeds">备用节点</Label>
        <Textarea id="redis-profile-seeds" v-model="form.seeds" class="redis-field-control text-xs" rows="2"
          :placeholder="sentinel ? '每行一个，例如 redis-sentinel-2:26379' : '每行一个，例如 redis-node-2:6379'" />
      </div>
      <section v-if="sentinel" aria-label="哨兵认证">
        <div class="form-item">
          <Label for="redis-profile-sentinel-service">主节点服务名</Label>
          <Input id="redis-profile-sentinel-service" v-model="form.sentinelService" class="redis-field-control"
            size="sm" placeholder="例如 mymaster" required />
        </div>
        <div class="form-item">
          <Label for="redis-profile-sentinel-user">哨兵用户</Label>
          <Input id="redis-profile-sentinel-user" v-model="form.sentinelUsername" class="redis-field-control" size="sm" />
        </div>
        <div class="form-item">
          <Label for="redis-profile-sentinel-password">哨兵密码</Label>
          <div class="redis-field-control grid justify-items-start gap-1">
            <Input id="redis-profile-sentinel-password" v-model="form.sentinelPassword" type="password" autocomplete="new-password"
              class="min-h-8" size="sm" :placeholder="retainedSentinelPassword ? '留空保留系统凭据' : '无密码可留空'" />
            <Button v-if="retainedSentinelPassword" size="xs" variant="ghost" type="button" @click="form.forgetSentinelPassword = true">移除已保存密码</Button>
            <Button v-else-if="form.forgetSentinelPassword" size="xs" variant="ghost" type="button" @click="form.forgetSentinelPassword = false">撤销移除密码</Button>
          </div>
        </div>
        <div class="form-item">
          <Label for="redis-profile-sentinel-tls">哨兵 TLS</Label>
          <div class="redis-field-control flex min-h-8 items-center">
            <Switch id="redis-profile-sentinel-tls" v-model="form.sentinelTls" size="sm" />
          </div>
        </div>
      </section>
      <div class="form-item">
        <Label for="redis-profile-user">{{ sentinel ? '数据节点用户' : '用户名' }}</Label>
        <Input id="redis-profile-user" v-model="form.username" class="redis-field-control" size="sm" placeholder="默认用户可留空" />
      </div>
      <div class="form-item">
        <Label for="redis-profile-password">{{ sentinel ? '数据节点密码' : '密码' }}</Label>
        <div class="redis-field-control grid justify-items-start gap-1">
          <Input id="redis-profile-password" v-model="form.password" type="password" autocomplete="new-password"
            class="min-h-8" size="sm" :placeholder="retainedPassword ? '留空保留系统凭据' : '无密码可留空'" />
          <Button v-if="retainedPassword" size="xs" variant="ghost" type="button" @click="form.forgetPassword = true">移除已保存密码</Button>
          <Button v-else-if="form.forgetPassword" size="xs" variant="ghost" type="button" @click="form.forgetPassword = false">撤销移除密码</Button>
        </div>
      </div>
      <div class="form-item">
        <Label for="redis-profile-db">默认 DB</Label>
        <Input id="redis-profile-db" :model-value="cluster ? 0 : form.db" class="redis-field-control" type="text" inputmode="numeric"
          size="sm" :disabled="cluster" required @update:model-value="form.db = $event" />
      </div>
      <div class="form-item">
        <Label for="redis-profile-separator">键分隔符</Label>
        <Input id="redis-profile-separator" v-model="form.keySeparator" class="redis-field-control" size="sm"
          autocomplete="off" placeholder="默认 :，留空使用平铺列表" />
      </div>
      <div class="form-item">
        <Label for="redis-profile-timeout">连接超时</Label>
        <Input id="redis-profile-timeout" v-model="form.connectTimeout" class="redis-field-control" type="text" inputmode="numeric"
          size="sm" placeholder="秒，例如 5" required />
      </div>
      <div class="form-item">
        <Label for="redis-profile-tls">{{ sentinel ? '数据节点 TLS' : 'TLS' }}</Label>
        <div class="redis-field-control flex min-h-8 items-center">
          <Switch id="redis-profile-tls" v-model="form.tls" size="sm" />
        </div>
      </div>
      <div v-if="!embedded" class="mt-3 flex flex-wrap gap-2 border-t border-border pt-3">
        <Button variant="outline" size="sm" type="button" :disabled="!!pending" @click="emit('cancel')">返回</Button>
        <Button variant="outline" size="sm" type="button" :disabled="!!pending" @click="submit('test')">{{ pending === 'test' ? '测试中…' : '测试连接' }}</Button>
        <Button size="sm" type="submit" :disabled="!!pending">{{ pending === 'save' ? '保存中…' : initial ? '保存修改' : '保存并连接' }}</Button>
      </div>
    </form>
  </section>
</template>

<style scoped src="../session/sessionFormRows.css"></style>

<style scoped>
.redis-profile-form .form-item > label {
  @apply block leading-5;
}

.redis-field-control {
  @apply w-full min-w-0 max-w-[280px];
}

</style>
