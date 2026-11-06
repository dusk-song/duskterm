<script setup>
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { Wrench } from '@lucide/vue';
import IconButton from '@/components/common/IconButton.vue';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Checkbox } from '@/components/ui/checkbox';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { aiInvoke } from '@/stores/ai';
import { useSecurityStore } from '@/stores/security';

const props = defineProps({ session: Object, modelId: String, serviceName: String, conversationId: String, visible: Boolean, busy: Boolean, diagnosisTarget: Object });
const emit = defineEmits(['change', 'focus', 'diagnose']);
const security = useSecurityStore();
const open = ref(false), path = ref(''), prepared = ref(null), enabled = ref(null), checking = ref(false), error = ref('');
const allowRead = ref(true), allowExec = ref(false);
const target = computed(() => props.session?.workspaceSessionId || props.session?.id || '');
const available = computed(() => !security.isLocked && !!target.value && props.session?.status === 'connected'
  && ['', 'ssh'].includes(props.session?.config?.protocol || '') && !!props.modelId);
let revision = 0, timer, disposed = false;
function revoke(scope) {
  if (scope?.token) void aiInvoke('ai_revoke_read_scope', { token: scope.token }).catch(() => {});
  if (scope?.execToken) void aiInvoke('ai_revoke_exec_scope', { token: scope.execToken }).catch(() => {});
}
function clear() {
  revision++; clearTimeout(timer); revoke(prepared.value); revoke(enabled.value);
  prepared.value = null; enabled.value = null; emit('change', null);
}
watch(() => [target.value, props.session?.id, props.session?.status, props.modelId, props.conversationId, props.visible, security.isLocked], () => {
  clear(); open.value = false; error.value = '';
});
watch(path, () => { revision++; revoke(prepared.value); prepared.value = null; error.value = ''; });
watch([allowRead, allowExec], () => { revision++; revoke(prepared.value); prepared.value = null; error.value = ''; });
watch(open, value => { if (!value) { revision++; revoke(prepared.value); prepared.value = null; } });
async function check() {
  if (!available.value || checking.value || (!allowRead.value && !allowExec.value) || (allowRead.value && !path.value.trim())) return;
  const current = ++revision;
  checking.value = true; error.value = '';
  revoke(prepared.value); prepared.value = null;
  try {
    const jobs = [];
    if (allowRead.value) jobs.push(aiInvoke('ai_prepare_read_scope', { target: target.value, path: path.value.trim(), modelId: props.modelId }).then(scope => ({ ...scope, execToken: null })));
    if (allowExec.value) jobs.push(aiInvoke('ai_prepare_exec_scope', { target: target.value, modelId: props.modelId }).then(scope => ({ token: null, execToken: scope.token, target: scope.target, expiresAt: scope.expiresAt, root: '', kind: 'commands' })));
    const results = await Promise.allSettled(jobs);
    const scopes = results.filter(result => result.status === 'fulfilled').map(result => result.value);
    const failure = results.find(result => result.status === 'rejected');
    if (failure) { scopes.forEach(revoke); throw failure.reason; }
    const scope = { ...scopes[0], execToken: scopes.find(scope => scope.execToken)?.execToken || null,
      expiresAt: Math.min(...scopes.map(scope => scope.expiresAt)) };
    if (disposed || current !== revision || !open.value) { revoke(scope); return; }
    prepared.value = scope;
  } catch (e) { if (current === revision) error.value = String(e); }
  finally { checking.value = false; }
}
function enable() {
  if (!prepared.value || prepared.value.expiresAt <= Date.now()) { clear(); error.value = '读取范围已过期，请重新检查'; return; }
  revoke(enabled.value); enabled.value = prepared.value; prepared.value = null;
  emit('diagnose', null);
  emit('change', enabled.value); open.value = false;
  clearTimeout(timer); timer = setTimeout(clear, Math.max(0, enabled.value.expiresAt - Date.now()));
}
function beginDiagnosis() {
  if (!available.value || props.busy) return;
  clear(); emit('diagnose', { id: target.value, label: String(props.session?.name || target.value).slice(0, 80) });
  open.value = false;
}
function consume() {
  const scope = enabled.value;
  if (!scope) return null;
  enabled.value = null; clearTimeout(timer); emit('change', null);
  if (scope.expiresAt <= Date.now()) { revoke(scope); throw new Error('读取范围已过期，请重新启用'); }
  return { readGrant: scope.token || null, execGrant: scope.execToken || null };
}
defineExpose({ consume, clear });
onBeforeUnmount(() => { disposed = true; clear(); });
</script>

<template>
  <IconButton :icon="Wrench" size="sm" :active="!!enabled || !!diagnosisTarget" :disabled="busy || !available"
    :aria-label="enabled ? '修改本次 SSH 工具' : '启用 SSH 工具'" tooltip-side="top" :action="() => { error = ''; open = true; }" />
  <Dialog v-model:open="open">
    <DialogContent @close-auto-focus="event => { if (enabled || diagnosisTarget) { event.preventDefault(); emit('focus'); } }">
      <DialogHeader>
        <DialogTitle>本次 SSH 工具</DialogTitle>
        <DialogDescription>当前目标：{{ session?.name || target }}。工具结果会发送到「{{ serviceName || '当前模型服务' }}」。</DialogDescription>
      </DialogHeader>
      <div class="diagnosis-entry">
        <p>直接描述排障目标，助手按证据分步提出检查命令，无需先勾选工具。每条实际命令仍需确认。</p>
        <Button :disabled="busy || !available" @click="beginDiagnosis">进入 SSH 诊断</Button>
        <Button v-if="diagnosisTarget" variant="ghost" @click="emit('diagnose', null); open = false">退出诊断模式</Button>
      </div>
      <details class="manual-tools">
        <summary>手动工具设置</summary>
      <div class="scope-fields">
        <p>目标：{{ session?.name || target }}</p>
        <div class="scope-option"><Checkbox id="ai-allow-read" v-model="allowRead" /><Label for="ai-allow-read">读取指定范围的文件</Label></div>
        <template v-if="allowRead">
          <Label for="ai-read-path">远程文件或目录</Label>
          <Input id="ai-read-path" v-model="path" placeholder="例如 /var/log/myapp" maxlength="4096" autocomplete="off" :disabled="busy" @keydown.enter.prevent="check" />
        </template>
        <div class="scope-option"><Checkbox id="ai-allow-exec" v-model="allowExec" /><Label for="ai-allow-exec">允许助手提出一条命令</Label></div>
        <p v-if="allowExec" class="muted">执行前审阅完整命令并逐条确认。使用远端 /bin/sh，非交互、30 秒超时；命令可以改变服务器，文件读取范围不限制命令作用。</p>
        <p class="muted">文件查询最多 8 次、64 KiB；不跟随符号链接。PowerShell / CMD 暂可带入终端输出。</p>
        <p v-if="prepared" class="resolved" role="status">{{ prepared.token ? `实际${prepared.kind === 'directory' ? '目录' : '文件'}：${prepared.root}` : '命令提议已就绪' }}<span v-if="prepared.execToken && prepared.token"> · 命令需逐条确认</span></p>
        <p v-if="error" class="scope-error" role="alert">{{ error }}</p>
      </div>
      <DialogFooter>
        <Button variant="ghost" @click="open = false">取消</Button>
        <Button variant="outline" :disabled="checking || !available || (!allowRead && !allowExec) || (allowRead && !path.trim())" @click="check">{{ checking ? '检查中…' : '检查范围' }}</Button>
        <Button :disabled="!prepared || checking || busy" @click="enable">启用本次工具</Button>
      </DialogFooter>
      </details>
    </DialogContent>
  </Dialog>
</template>

<style scoped>
.diagnosis-entry{display:flex;flex-direction:column;gap:10px;font-size:12px;line-height:1.7}.manual-tools{border-top:1px solid var(--app-border-light);padding-top:10px}.manual-tools>summary{cursor:pointer;font-size:12px;color:var(--app-text-muted);margin-bottom:12px}.manual-tools :deep([data-slot=dialog-footer]){margin-top:12px}
.scope-option{display:flex;align-items:center;gap:8px}
.scope-fields{display:flex;flex-direction:column;gap:10px;min-width:0;font-size:12px;overflow-wrap:anywhere}.muted{color:var(--app-text-muted);font-size:11px;line-height:1.6}.resolved{padding:8px;border:1px solid var(--app-border-light);border-radius:8px}.scope-error{color:var(--color-destructive,#e16b6b);font-size:12px}
</style>
