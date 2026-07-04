<script setup>
import Button from '@/components/ui/button/Button.vue';
import Checkbox from '@/components/ui/checkbox/Checkbox.vue';
import Dialog from '@/components/ui/dialog/Dialog.vue';
import DialogContent from '@/components/ui/dialog/DialogContent.vue';
import DialogFooter from '@/components/ui/dialog/DialogFooter.vue';
import DialogHeader from '@/components/ui/dialog/DialogHeader.vue';
import DialogTitle from '@/components/ui/dialog/DialogTitle.vue';
import Input from '@/components/ui/input/Input.vue';
import Select from '@/components/ui/select/Select.vue';
import SelectContent from '@/components/ui/select/SelectContent.vue';
import SelectItem from '@/components/ui/select/SelectItem.vue';
import SelectTrigger from '@/components/ui/select/SelectTrigger.vue';
import SelectValue from '@/components/ui/select/SelectValue.vue';
import Textarea from '@/components/ui/textarea/Textarea.vue';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { toast } from '@/composables/useToast';
import { open } from '@tauri-apps/plugin-dialog';
import {
  Code2,
  Eye,
  FolderOpen,
  Info,
  Server,
  Settings,
  Zap
} from '@lucide/vue';
import { v4 as uuidv4 } from 'uuid';
import { computed, reactive, ref, watch } from 'vue';
import { useSshStore } from '@/stores/ssh';
import { invokeCommand } from '@/utils/ipc';

const props = defineProps({
  visible: Boolean,
  sessionData: Object
});

const emit = defineEmits(['update:visible', 'saved']);

const dialogOpen = computed({
  get: () => props.visible,
  set: (v) => { if (!v) handleCancel(); },
});

const sshStore = useSshStore();
const confirmLoading = ref(false);
const isTestingConnection = ref(false);
const activeProtocol = ref('ssh');
const activeConfigTab = ref('basic');
const serialPortOptions = ref([]);

const protocolOptions = [
  { key: 'ssh', label: 'SSH', desc: '标准终端连接' },
  { key: 'telnet', label: 'Telnet', desc: '兼容老设备与网络设备', disabled: false },
  { key: 'serial', label: '串口', desc: '连接 COM / ttyUSB / ttyS 设备', disabled: false }
];

const sectionTips = {
  basic: '根据协议填写主机或串口等基础信息',
  auth: 'SSH 支持密码/私钥，Telnet 支持密码辅助登录',
  terminal: '终端外观与字符编码设置',
  connection: '连接超时与保活配置',
  advanced: '登录脚本、跳板机、代理等高级能力'
};

const validateRequiredIntegerRange = (fieldName, min, max) => async (_rule, value) => {
  const text = String(value ?? '').trim();
  if (!text) {
    return Promise.reject(`请输入${fieldName}`);
  }
  if (!/^\d+$/.test(text)) {
    return Promise.reject(`${fieldName}必须是整数`);
  }
  const num = Number(text);
  if (num < min || num > max) {
    return Promise.reject(`${fieldName}范围 ${min}-${max}`);
  }
  return Promise.resolve();
};

// --- Form State ---
const formState = reactive({
  id: '',
  protocol: 'ssh',
  // Basic
  name: '',
  host: '',
  port: 22,
  username: '',
  auth_type: 'password', // 'password' | 'key'
  password: '',
  private_key_path: '',
  passphrase: '',
  group: '',
  remarks: '',

  // Terminal
  term_type: 'xterm-256color',
  encoding: 'UTF-8', // Added to requirements
  font_size: 14,
  font_family: 'Consolas', // Simplified for now

  // Connection
  connect_timeout: 10,
  keep_alive_interval: 30, // 0 to disable

  // Advanced
  local_forward: '', // string "local:remote_ip:remote_port" multiple lines
  remote_forward: '',
  proxy_type: 'none', // none, socks5, http
  proxy_host: '',
  proxy_port: 1080,
  proxy_auth: false,
  proxy_user: '',
  proxy_pass: '',
  jump_host: '',
  jump_port: 22,
  jump_username: '',
  jump_auth_type: 'password',
  jump_password: '',
  jump_private_key_path: '',
  jump_passphrase: '',
  login_script: '', // multiline string
  serial_path: '',
  baud_rate: 9600,
  data_bits: 8,
  stop_bits: '1',
  parity: 'none',
  flow_control: 'none'
});

// --- Validation Rules ---
const rules = {
  name: [{ required: true, message: '请输入会话名称', trigger: 'blur' }],
  host: [
    { required: true, message: '请输入主机地址', trigger: 'blur' },
    { pattern: /^(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)$|^(([a-zA-Z0-9]|[a-zA-Z0-9][a-zA-Z0-9\-]*[a-zA-Z0-9])\.)+([A-Za-z]|[A-Za-z][A-Za-z0-9\-]*[A-Za-z0-9])$|localhost/, message: '请输入有效的IP或域名', trigger: 'blur' }
  ],
  port: [{ validator: validateRequiredIntegerRange('端口', 1, 65535), trigger: 'blur' }],
  username: [{ required: true, message: '请输入用户名', trigger: 'blur' }],
  password: [{ required: true, message: '请输入密码', trigger: 'blur' }],
  private_key_path: [{ required: true, message: '请选择私钥文件', trigger: 'change' }],
  connect_timeout: [{ validator: validateRequiredIntegerRange('连接超时', 1, 120), trigger: 'blur' }],
  keep_alive_interval: [{ validator: validateRequiredIntegerRange('心跳间隔', 0, 3600), trigger: 'blur' }],
  proxy_host: [{ required: true, message: '请输入代理主机', trigger: 'blur' }],
  proxy_port: [{ validator: validateRequiredIntegerRange('代理端口', 1, 65535), trigger: 'blur' }],
  jump_port: [{ validator: validateRequiredIntegerRange('跳板机端口', 1, 65535), trigger: 'blur' }],
  serial_path: [{ required: true, message: '请选择或输入串口设备', trigger: 'blur' }],
  baud_rate: [{ validator: validateRequiredIntegerRange('波特率', 50, 921600), trigger: 'blur' }]
};

const isSshProtocol = computed(() => activeProtocol.value === 'ssh');
const isTelnetProtocol = computed(() => activeProtocol.value === 'telnet');
const isSerialProtocol = computed(() => activeProtocol.value === 'serial');
const supportsHostPort = computed(() => !isSerialProtocol.value);

const configTabs = [
  { key: 'basic', label: '基础连接' },
  { key: 'terminal', label: '终端设置' },
  { key: 'connection', label: '连接优化' },
  { key: 'advanced', label: '高级功能' },
];

// Dynamic validation for auth fields
const isEditMode = computed(() => !!props.sessionData?.id);
const computedRules = computed(() => {
  const r = { ...rules };
  if (isSshProtocol.value && formState.auth_type === 'password') {
    // Only require password in create mode; edit mode allows empty = keep existing
    r.password = isEditMode.value
      ? []
      : [{ required: true, message: '请输入密码', trigger: 'blur' }];
    delete r.private_key_path;
    delete r.passphrase;
  } else if (isSshProtocol.value) {
    r.private_key_path = [{ required: true, message: '请选择私钥文件', trigger: 'change' }];
    delete r.password;
    delete r.passphrase;
  } else {
    delete r.password;
    delete r.private_key_path;
    delete r.passphrase;
  }
  if (!supportsHostPort.value) {
    delete r.host;
    delete r.port;
    delete r.username;
  } else if (isTelnetProtocol.value) {
    delete r.username;
  }
  if (formState.proxy_type === 'none') {
    delete r.proxy_host;
    delete r.proxy_port;
  }
  if (!isSshProtocol.value || !String(formState.jump_host || '').trim()) {
    delete r.jump_port;
  }
  if (!isSerialProtocol.value) {
    delete r.serial_path;
    delete r.baud_rate;
  }
  return r;
});

const formRef = ref();

// --- Lifecycle ---
watch(() => props.visible, (val) => {
  if (val) {
    activeConfigTab.value = 'basic';
    if (props.sessionData) {
      // Edit Mode — copy all fields EXCEPT passwords (security: never pre-fill)
      const defaultState = getNewSessionState();
      const data = JSON.parse(JSON.stringify(props.sessionData));
      // Clear sensitive fields — user must re-enter to change
      data.password = '';
      data.passphrase = '';
      data.jump_password = '';
      data.jump_passphrase = '';
      Object.assign(formState, defaultState, data);
      activeProtocol.value = formState.protocol || 'ssh';
    } else {
      // Create Mode
      Object.assign(formState, getNewSessionState());
      activeProtocol.value = 'ssh';
    }

    if (activeProtocol.value === 'serial') {
      loadSerialPortOptions();
    }
  }
});

watch(activeProtocol, (value) => {
  formState.protocol = value;
  if (value === 'serial') {
    loadSerialPortOptions();
    formState.host = '';
    formState.port = 0;
    formState.username = '';
  } else if (!formState.port) {
    formState.port = value === 'telnet' ? 23 : 22;
  }
});

function getNewSessionState() {
  return {
    id: uuidv4(),
    protocol: 'ssh',
    name: '',
    host: '',
    port: 22,
    username: 'root',
    auth_type: 'password',
    password: '',
    private_key_path: '',
    passphrase: '',
    group: '',
    remarks: '',
    term_type: 'xterm-256color',
    encoding: 'UTF-8',
    font_size: 14,
    font_family: 'Consolas',
    connect_timeout: 10,
    keep_alive_interval: 30,
    local_forward: '',
    remote_forward: '',
    proxy_type: 'none',
    proxy_host: '',
    proxy_port: 1080,
    proxy_auth: false,
    proxy_user: '',
    proxy_pass: '',
    jump_host: '',
    jump_port: 22,
    jump_username: '',
    jump_auth_type: 'password',
    jump_password: '',
    jump_private_key_path: '',
    jump_passphrase: '',
    login_script: '',
    serial_path: '',
    baud_rate: 9600,
    data_bits: 8,
    stop_bits: '1',
    parity: 'none',
    flow_control: 'none'
  };
}

const loadSerialPortOptions = async () => {
  try {
    const ports = await invokeCommand('list_serial_ports');
    serialPortOptions.value = Array.isArray(ports)
      ? ports.map((item) => ({ value: item.path, label: item.label || item.path }))
      : [];
  } catch (error) {
    console.error('Load serial ports failed:', error);
    serialPortOptions.value = [];
  }
};

const normalizeOptional = (value) => {
  if (value === null || value === undefined) return null;
  if (typeof value === 'string' && value.trim() === '') return null;
  return value;
};

const buildSessionConfig = () => {
  const sessionConfig = JSON.parse(JSON.stringify(formState));

  // In edit mode: if password/passphrase fields are empty, preserve the original stored values
  if (props.sessionData?.id) {
    if (!(sessionConfig.password || '').trim()) {
      sessionConfig.password = props.sessionData.password || null;
    }
    if (!(sessionConfig.passphrase || '').trim()) {
      sessionConfig.passphrase = props.sessionData.passphrase || null;
    }
    if (!(sessionConfig.jump_password || '').trim()) {
      sessionConfig.jump_password = props.sessionData.jump_password || null;
    }
    if (!(sessionConfig.jump_passphrase || '').trim()) {
      sessionConfig.jump_passphrase = props.sessionData.jump_passphrase || null;
    }
  }

  sessionConfig.protocol = activeProtocol.value;
  sessionConfig.port = Number(sessionConfig.port);
  sessionConfig.connect_timeout = Number(sessionConfig.connect_timeout);
  sessionConfig.keep_alive_interval = Number(sessionConfig.keep_alive_interval);
  sessionConfig.proxy_port = Number(sessionConfig.proxy_port);
  sessionConfig.jump_port = Number(sessionConfig.jump_port);
  sessionConfig.baud_rate = Number(sessionConfig.baud_rate);
  sessionConfig.data_bits = Number(sessionConfig.data_bits);

  if (activeProtocol.value === 'ssh' && formState.auth_type === 'password') {
    sessionConfig.private_key_path = null;
    sessionConfig.passphrase = null;
  } else if (activeProtocol.value === 'ssh') {
    sessionConfig.password = null;
  } else {
    sessionConfig.auth_type = 'password';
    sessionConfig.private_key_path = null;
    sessionConfig.passphrase = null;
  }

  if (activeProtocol.value !== 'ssh' || formState.proxy_type === 'none') {
    sessionConfig.proxy_host = null;
    sessionConfig.proxy_port = null;
    sessionConfig.proxy_user = null;
    sessionConfig.proxy_pass = null;
    sessionConfig.proxy_auth = false;
  }

  if (activeProtocol.value !== 'ssh' || !String(formState.jump_host || '').trim()) {
    sessionConfig.jump_host = null;
    sessionConfig.jump_port = null;
    sessionConfig.jump_username = null;
    sessionConfig.jump_auth_type = 'password';
    sessionConfig.jump_password = null;
    sessionConfig.jump_private_key_path = null;
    sessionConfig.jump_passphrase = null;
  } else if (formState.jump_auth_type === 'password') {
    sessionConfig.jump_private_key_path = null;
    sessionConfig.jump_passphrase = null;
  } else {
    sessionConfig.jump_password = null;
  }

  if (activeProtocol.value === 'serial') {
    sessionConfig.host = '';
    sessionConfig.port = 0;
    sessionConfig.username = '';
    sessionConfig.password = null;
    sessionConfig.local_forward = null;
    sessionConfig.remote_forward = null;
    sessionConfig.keep_alive_interval = 0;
  } else {
    sessionConfig.serial_path = null;
    sessionConfig.baud_rate = null;
    sessionConfig.data_bits = null;
    sessionConfig.stop_bits = null;
    sessionConfig.parity = null;
    sessionConfig.flow_control = null;
  }

  if (activeProtocol.value === 'telnet') {
    sessionConfig.auth_type = 'password';
    sessionConfig.private_key_path = null;
    sessionConfig.passphrase = null;
    sessionConfig.local_forward = null;
    sessionConfig.remote_forward = null;
    sessionConfig.proxy_type = 'none';
    sessionConfig.jump_host = null;
    sessionConfig.jump_port = null;
    sessionConfig.jump_username = null;
    sessionConfig.jump_auth_type = null;
    sessionConfig.jump_password = null;
    sessionConfig.jump_private_key_path = null;
    sessionConfig.jump_passphrase = null;
  }

  sessionConfig.password = normalizeOptional(sessionConfig.password);
  sessionConfig.private_key_path = normalizeOptional(sessionConfig.private_key_path);
  sessionConfig.passphrase = normalizeOptional(sessionConfig.passphrase);
  sessionConfig.remarks = normalizeOptional(sessionConfig.remarks);
  sessionConfig.group = normalizeOptional(sessionConfig.group);
  sessionConfig.term_type = normalizeOptional(sessionConfig.term_type);
  sessionConfig.encoding = normalizeOptional(sessionConfig.encoding);
  sessionConfig.font_family = normalizeOptional(sessionConfig.font_family);
  sessionConfig.local_forward = normalizeOptional(sessionConfig.local_forward);
  sessionConfig.remote_forward = normalizeOptional(sessionConfig.remote_forward);
  sessionConfig.proxy_type = normalizeOptional(sessionConfig.proxy_type) || 'none';
  sessionConfig.proxy_host = normalizeOptional(sessionConfig.proxy_host);
  sessionConfig.proxy_user = normalizeOptional(sessionConfig.proxy_user);
  sessionConfig.proxy_pass = normalizeOptional(sessionConfig.proxy_pass);
  sessionConfig.jump_host = normalizeOptional(sessionConfig.jump_host);
  sessionConfig.jump_username = normalizeOptional(sessionConfig.jump_username);
  sessionConfig.jump_auth_type = normalizeOptional(sessionConfig.jump_auth_type) || 'password';
  sessionConfig.jump_password = normalizeOptional(sessionConfig.jump_password);
  sessionConfig.jump_private_key_path = normalizeOptional(sessionConfig.jump_private_key_path);
  sessionConfig.jump_passphrase = normalizeOptional(sessionConfig.jump_passphrase);
  sessionConfig.login_script = normalizeOptional(sessionConfig.login_script);
  sessionConfig.serial_path = normalizeOptional(sessionConfig.serial_path);
  sessionConfig.stop_bits = normalizeOptional(sessionConfig.stop_bits) || '1';
  sessionConfig.parity = normalizeOptional(sessionConfig.parity) || 'none';
  sessionConfig.flow_control = normalizeOptional(sessionConfig.flow_control) || 'none';
  return sessionConfig;
};

// --- Actions ---
const handleSelectKeyFile = async () => {
  try {
    const selected = await open({
      multiple: false,
      filters: [{
        name: 'SSH Key',
        extensions: ['pem', 'ppk', 'rsa', 'dsa', 'key', 'openssh', 'private']
      }]
    });
    if (selected) {
      formState.private_key_path = selected.path || selected; // plugin-dialog v2 returns object or string depending
    }
  } catch (err) {
    toast.error('无法打开文件选择框: ' + err);
  }
};

const handleSelectJumpKeyFile = async () => {
  try {
    const selected = await open({
      multiple: false,
      filters: [{
        name: 'SSH Key',
        extensions: ['pem', 'ppk', 'rsa', 'dsa', 'key', 'openssh', 'private']
      }]
    });
    if (selected) {
      formState.jump_private_key_path = selected.path || selected;
    }
  } catch (e) {
    toast.error(`选择跳板机私钥失败: ${e}`);
  }
};

const handleOk = async () => {
  // Code validation for the local form controls
  if (supportsHostPort.value) {
    if (!formState.host?.trim()) { toast.error('请输入主机地址'); return; }
    if (!formState.port || formState.port < 1 || formState.port > 65535) { toast.error('端口范围 1-65535'); return; }
  }
  if (isSshProtocol.value && !formState.username?.trim()) { toast.error('请输入用户名'); return; }
  if (isSshProtocol.value && formState.auth_type === 'password' && !isEditMode.value && !formState.password) { toast.error('请输入密码'); return; }
  if (isSshProtocol.value && formState.auth_type === 'key' && !formState.private_key_path?.trim()) { toast.error('请选择私钥文件'); return; }
  if (isSerialProtocol.value && !formState.serial_path?.trim()) { toast.error('请选择串口设备'); return; }

  confirmLoading.value = true;
  try {
    const sessionConfig = buildSessionConfig();

    // Preserve last_connected if editing
    if (props.sessionData && props.sessionData.last_connected) {
      sessionConfig.last_connected = props.sessionData.last_connected;
    }

    const saved = await sshStore.saveSessionToStorage(sessionConfig);
    if (!saved) return;
    emit('update:visible', false);
    emit('saved');
  } catch (e) {
    toast.error('保存失败: ' + e);
    console.error(e);
  } finally {
    confirmLoading.value = false;
  }
};

const handleCancel = () => {
  emit('update:visible', false);
};

const handleTestConnection = async () => {
  if (isTestingConnection.value) return;
  if (supportsHostPort.value && !formState.host?.trim()) { toast.error('请输入主机地址'); return; }

  const loadingKey = 'test-conn';
  isTestingConnection.value = true;
  toast.loading({ content: '正在测试端口连通性…', key: loadingKey, duration: 0 });

  try {
    const testConfig = buildSessionConfig();
    // Use the Rust command which now does TCP-only test for all protocols
    const result = await invokeCommand('test_ssh_connection', { config: testConfig });
    toast.success({ content: typeof result === 'string' ? result : '端口连通性正常', key: loadingKey });
  } catch (error) {
    const text = String(error || '端口不可达');
    toast.error({ content: text, key: loadingKey, duration: 4 });
  } finally {
    isTestingConnection.value = false;
  }
};

const groupOptions = computed(() => {
  const groups = new Set(sshStore.savedSessions.map(s => s.group).filter(Boolean));
  return Array.from(groups).map(g => ({ value: g }));
});
</script>

<template>
  <Dialog v-model:open="dialogOpen" modal>
    <DialogContent showCloseButton
      class="flex h-[min(500px,calc(100vh-4rem))] max-h-[calc(100vh-4rem)] w-[640px] max-w-[90vw] flex-col sm:max-w-[90vw]"
      @pointer-down-outside.prevent @interact-outside.prevent>
      <DialogHeader>
        <DialogTitle>{{ sessionData ? '编辑会话' : '新建会话' }}</DialogTitle>
      </DialogHeader>
      <div class="flex-1 min-h-0 overflow-y-auto px-4 py-2">
        <div class="protocol-tabs mb-3">
          <div class="flex gap-1 mb-1">
            <button v-for="item in protocolOptions" :key="item.key"
              :class="['px-3 py-1 rounded text-sm font-medium outline-none transition-[background,color,box-shadow]', 'focus-visible:bg-[var(--app-focus-bg)] focus-visible:text-foreground focus-visible:shadow-[var(--app-focus-shadow)]', activeProtocol === item.key ? 'bg-primary text-primary-foreground' : 'bg-secondary text-secondary-foreground hover:bg-accent']"
              :disabled="item.disabled" type="button" @click="activeProtocol = item.key">{{ item.label }}</button>
          </div>
        </div>

        <div layout="horizontal" class="session-compact-form" autocomplete="off" :model="formState" name="sessionForm">
          <div class="config-tabs flex">
            <div class="config-tab-sidebar flex flex-col gap-0.5 w-24 shrink-0 pr-3 border-r border-border">
              <button v-for="tab in configTabs" :key="tab.key" type="button"
                :class="['px-2 py-1.5 rounded text-sm text-left outline-none transition-[background,color,box-shadow]', 'focus-visible:bg-[var(--app-focus-bg)] focus-visible:text-foreground focus-visible:shadow-[var(--app-focus-shadow)]', activeConfigTab === tab.key ? 'bg-primary/15 text-primary font-semibold' : 'text-muted-foreground hover:text-foreground']"
                @click="activeConfigTab = tab.key">{{ tab.label }}</button>
            </div>
            <div v-show="activeConfigTab === 'basic'">
              <div class="compact-form-grid">
                <div class="form-item mb-2" name="name"><label class="text-sm text-muted-foreground mb-1">会话名称</label>
                  <Input size="sm" class="w-[280px] flex-none" v-model="formState.name" placeholder="例如: 生产环境服务器"
                    autofocus />
                </div>
                <div class="form-item mb-2" name="group"><label class="text-sm text-muted-foreground mb-1">分组标签</label>
                  <Input size="sm" class="w-[280px] flex-none" v-model="formState.group" placeholder="例如: Production" />
                </div>
                <div class="form-item mb-2" v-if="supportsHostPort" name="host">
                  <label class="text-sm text-muted-foreground mb-1">主机地址</label>
                  <Input size="sm" class="w-[280px] flex-none" v-model="formState.host"
                    placeholder="192.168.1.1 或 example.com" />
                </div>
                <div class="form-item mb-2" v-if="supportsHostPort" name="port">
                  <label class="text-sm text-muted-foreground mb-1">端口</label>
                  <Input size="sm" class="w-[280px] flex-none" v-model="formState.port"
                    :placeholder="isTelnetProtocol ? '23' : '22'" />
                </div>
                <div class="form-item mb-2" v-if="isSshProtocol || isTelnetProtocol"
                  :name="isSshProtocol ? 'username' : undefined">
                  <label class="text-sm text-muted-foreground mb-1">用户名</label>
                  <Input size="sm" class="w-[280px] flex-none" v-model="formState.username"
                    :placeholder="isTelnetProtocol ? '可选，便于自动填充 login' : 'root'" />
                </div>
                <template v-if="isSerialProtocol">
                  <div class="form-item mb-2" name="serial_path"><label
                      class="text-sm text-muted-foreground mb-1">串口设备</label>
                    <Input size="sm" class="w-[280px] flex-none" v-model="formState.serial_path"
                      placeholder="例如: COM3 / /dev/ttyUSB0 / /dev/ttyS0" />
                  </div>
                  <div class="form-item mb-2" name="baud_rate"><label
                      class="text-sm text-muted-foreground mb-1">波特率</label>
                    <Input size="sm" class="w-[280px] flex-none" v-model="formState.baud_rate" placeholder="9600" />
                  </div>
                  <div class="form-item mb-2"><label class="text-sm text-muted-foreground mb-1">数据位</label>
                    <Select v-model="formState.data_bits">
                      <SelectTrigger size="sm" class="w-[280px] flex-none">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent :side-offset="4">
                        <SelectItem value="5">5</SelectItem>
                        <SelectItem value="6">6</SelectItem>
                        <SelectItem value="7">7</SelectItem>
                        <SelectItem value="8">8</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <div class="form-item mb-2"><label class="text-sm text-muted-foreground mb-1">停止位</label>
                    <Select v-model="formState.stop_bits">
                      <SelectTrigger size="sm" class="w-[280px] flex-none">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent :side-offset="4">
                        <SelectItem value="1">1</SelectItem>
                        <SelectItem value="2">2</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <div class="form-item mb-2"><label class="text-sm text-muted-foreground mb-1">校验位</label>
                    <Select v-model="formState.parity">
                      <SelectTrigger size="sm" class="w-[280px] flex-none">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent :side-offset="4">
                        <SelectItem value="none">无</SelectItem>
                        <SelectItem value="odd">奇校验</SelectItem>
                        <SelectItem value="even">偶校验</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <div class="form-item mb-2"><label class="text-sm text-muted-foreground mb-1">流控</label>
                    <Select v-model="formState.flow_control">
                      <SelectTrigger size="sm" class="w-[280px] flex-none">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent :side-offset="4">
                        <SelectItem value="none">无</SelectItem>
                        <SelectItem value="software">软件流控 XON/XOFF</SelectItem>
                        <SelectItem value="hardware">硬件流控 RTS/CTS</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                </template>
                <!-- SSH 认证：支持密码 / 私钥 -->
                <template v-if="isSshProtocol">
                  <div class="form-item mb-2" name="auth_type"><label
                      class="text-sm text-muted-foreground mb-1">认证方式</label>
                    <div class="flex gap-1">
                      <button type="button"
                        :class="['px-3 py-1 rounded text-sm font-medium outline-none transition-[background,color,box-shadow]', 'focus-visible:bg-[var(--app-focus-bg)] focus-visible:text-foreground focus-visible:shadow-[var(--app-focus-shadow)]', formState.auth_type === 'password' ? 'bg-primary text-primary-foreground' : 'bg-secondary text-secondary-foreground hover:bg-accent']"
                        @click="formState.auth_type = 'password'">
                        <Eye :size="14" class="auth-icon inline" />
                        <span>密码认证</span>
                      </button>
                      <button type="button"
                        :class="['px-3 py-1 rounded text-sm font-medium outline-none transition-[background,color,box-shadow]', 'focus-visible:bg-[var(--app-focus-bg)] focus-visible:text-foreground focus-visible:shadow-[var(--app-focus-shadow)]', formState.auth_type === 'key' ? 'bg-primary text-primary-foreground' : 'bg-secondary text-secondary-foreground hover:bg-accent']"
                        @click="formState.auth_type = 'key'">
                        <FolderOpen :size="14" class="auth-icon inline" />
                        <span>私钥认证</span>
                      </button>
                    </div>
                  </div>
                  <template v-if="formState.auth_type === 'password'">
                    <div class="form-item mb-2" label="密码" name="password">
                      <label class="text-sm text-muted-foreground mb-1">密码</label>
                      <Input size="sm" class="w-[280px] flex-none" v-model="formState.password" type="password"
                        placeholder="留空则不修改密码" autocomplete="new-password" />
                    </div>
                  </template>
                  <template v-else>
                    <div class="form-item mb-2" name="private_key_path">
                      <label class="text-sm text-muted-foreground mb-1">路径</label>
                      <Input size="sm" class="w-[280px] flex-none" v-model="formState.private_key_path"
                        placeholder="选择 .pem, .ppk 或其他私钥文件" readonly />
                      <Button variant="ghost" size="sm" @click="handleSelectKeyFile" class="mt-1">
                        <FolderOpen :size="14" /> 浏览
                      </Button>
                    </div>
                    <div class="form-item mb-2" name="passphrase"><label
                        class="text-sm text-muted-foreground mb-1">口令</label>
                      <Input size="sm" class="w-[280px] flex-none" v-model="formState.passphrase" type="password"
                        placeholder="留空则不修改" autocomplete="new-password" />
                    </div>
                  </template>
                </template>
                <!-- Telnet 认证：仅密码 -->
                <template v-if="isTelnetProtocol">
                  <div class="form-item mb-2">
                    <label class="text-sm text-muted-foreground mb-1">密码</label>
                    <Input size="sm" class="w-[280px] flex-none" v-model="formState.password" type="password"
                      placeholder="如设备提示 password: 则自动填入" autocomplete="new-password" />
                  </div>
                  <!-- <div class="field-hint">Telnet 不提供安全加密，建议仅在可信网络或旧设备维护场景使用。</div> -->
                </template>
                <div class="form-item mb-2" name="remarks"><label
                    class="text-sm text-muted-foreground mb-1">备注说明</label>
                  <Textarea v-model="formState.remarks" :rows="2" placeholder="添加关于此会话的备注信息..." :maxlength="700"
                    autocomplete="off" class="session-textarea" />
                </div>
              </div>
            </div>

            <div v-show="activeConfigTab === 'terminal'">
              <div class="compact-form-grid">
                <div class="form-item mb-2"><label class="text-sm text-muted-foreground mb-1">终端类型</label>
                  <Select v-model="formState.term_type">
                    <SelectTrigger size="sm" class="w-[280px] flex-none">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent :side-offset="4">
                      <SelectItem value="xterm">xterm</SelectItem>
                      <SelectItem value="xterm-256color">xterm-256color (推荐)</SelectItem>
                      <SelectItem value="vt100">vt100</SelectItem>
                      <SelectItem value="linux">linux</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
                <div class="form-item mb-2"><label class="text-sm text-muted-foreground mb-1">字符编码</label>
                  <Select v-model="formState.encoding">
                    <SelectTrigger size="sm" class="w-[280px] flex-none">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent :side-offset="4">
                      <SelectItem value="UTF-8">UTF-8</SelectItem>
                      <SelectItem value="GBK">GBK</SelectItem>
                      <SelectItem value="GB2312">GB2312</SelectItem>
                      <SelectItem value="ISO-8859-1">ISO-8859-1</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
                <div class="form-item mb-2"><label class="text-sm text-muted-foreground mb-1">字体</label>
                  <Select v-model="formState.font_family">
                    <SelectTrigger size="sm" class="w-[280px] flex-none">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent :side-offset="4">
                      <SelectItem value="Consolas">Consolas</SelectItem>
                      <SelectItem value="'Courier New'">Courier New</SelectItem>
                      <SelectItem value="'Fira Code'">Fira Code</SelectItem>
                      <SelectItem value="monospace">System Monospace</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
                <div class="form-item mb-2"><label class="text-sm text-muted-foreground mb-1">字号</label>
                  <Input size="sm" class="w-[280px] flex-none" v-model="formState.font_size" placeholder="14" />
                </div>
              </div>
            </div>

            <div v-show="activeConfigTab === 'connection'">
              <div class="compact-form-grid">
                <div class="form-item mb-2" name="connect_timeout">
                  <label class="text-sm text-muted-foreground mb-1">连接超时</label>
                  <Input size="sm" class="w-[280px] flex-none" v-model="formState.connect_timeout" placeholder="秒" />
                  <!-- <div class="field-hint">网络条件较差时建议增加此值</div> -->
                </div>
                <div class="form-item mb-2" v-if="!isSerialProtocol" label="心跳间隔" name="keep_alive_interval">
                  <label class="text-sm text-muted-foreground mb-1">心跳间隔</label>
                  <Input size="sm" class="w-[280px] flex-none" v-model="formState.keep_alive_interval"
                    placeholder="秒" />
                  <!-- <div class="field-hint">SSH / Telnet 可选保活 (0 为禁用)</div> -->
                </div>
              </div>
            </div>

            <div v-show="activeConfigTab === 'advanced'">
              <div class="section-header">
                <div class="section-title-row">
                  <h3>高级功能</h3>
                  <Tooltip>
                    <TooltipTrigger>
                      <Info class="section-tip-icon" />
                    </TooltipTrigger>
                    <TooltipContent>
                      {{ sectionTips.advanced }}
                    </TooltipContent>
                  </Tooltip>
                </div>
              </div>
              <div class="advanced-section">
                <div class="advanced-card">
                  <div class="card-header">
                    <Code2 class="card-icon" />
                    <h4>登录脚本</h4>
                  </div>
                  <div class="form-item mb-2"><label class="text-sm text-muted-foreground mb-1">登录脚本</label>
                    <Textarea v-model="formState.login_script" rows="4" autocomplete="off"
                      placeholder="cd /var/www&#10;ls -la&#10;# 登录成功后自动执行的命令 (每行一条)" class="session-textarea" />
                  </div>
                </div>

                <div v-if="isSshProtocol" class="advanced-card">
                  <div class="card-header">
                    <Settings class="card-icon" />
                    <h4>端口转发</h4>
                  </div>
                  <div class="form-item mb-2">
                    <label class="text-sm text-muted-foreground mb-1">本地转发</label>
                    <input v-model="formState.local_forward" :rows="2" autocomplete="off" placeholder="本地端口:远程地址:远程端口"
                      class="session-textarea">

                  </div>
                  <div class="form-item mb-2">
                    <label class="text-sm text-muted-foreground mb-1">远程转发</label>
                    <input v-model="formState.remote_forward" :rows="2" autocomplete="off" placeholder="远程端口:本地地址:本地端口"
                      class="session-textarea">

                  </div>
                </div>

                <div v-if="isSshProtocol" class="advanced-card">
                  <div class="card-header">
                    <Zap class="card-icon" />
                    <h4>代理服务器</h4>
                  </div>
                  <div class="form-item mb-2"><label class="text-sm text-muted-foreground mb-1">代理类型</label>
                    <Select v-model="formState.proxy_type">
                      <SelectTrigger size="sm" class="w-[280px] flex-none">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent :side-offset="4">
                        <SelectItem value="none">无代理</SelectItem>
                        <SelectItem value="socks5">SOCKS5</SelectItem>
                        <SelectItem value="http">HTTP</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  <div v-if="formState.proxy_type !== 'none'" class="proxy-config">
                    <div class="form-item mb-2" name="proxy_host"><label
                        class="text-sm text-muted-foreground mb-1">代理主机</label>
                      <Input size="sm" class="w-[280px] flex-none" v-model="formState.proxy_host"
                        placeholder="127.0.0.1" />
                    </div>
                    <div class="form-item mb-2" name="proxy_port"><label
                        class="text-sm text-muted-foreground mb-1">端口</label>
                      <Input size="sm" v-model="formState.proxy_port" placeholder="1080" />
                    </div>
                    <div class="form-item mb-2">
                      <label class="text-sm text-muted-foreground mb-1">认证</label>
                      <label class="inline-flex items-center gap-2 cursor-pointer">
                        <Checkbox v-model="formState.proxy_auth" />
                        <span class="text-sm">需要认证</span>
                      </label>
                    </div>

                  </div>
                  <template v-if="formState.proxy_auth">
                    <div class="form-item mb-2"><label class="text-sm text-muted-foreground mb-1">用户名</label>
                      <Input size="sm" v-model="formState.proxy_user" />
                    </div>
                    <div class="form-item mb-2"><label class="text-sm text-muted-foreground mb-1">密码</label>
                      <Input size="sm" class="w-[280px] flex-none" v-model="formState.proxy_pass" type="password"
                        autocomplete="new-password" />
                    </div>
                  </template>
                </div>

                <div v-if="isSshProtocol" class="advanced-card">
                  <div class="card-header">
                    <Server class="card-icon" />
                    <h4>跳板机 / 堡垒机</h4>
                  </div>
                  <div class="form-item mb-2" name="jump_host"><label
                      class="text-sm text-muted-foreground mb-1">主机</label>
                    <Input size="sm" class="w-[280px] flex-none" v-model="formState.jump_host"
                      placeholder="例如 bastion.example.com" />
                  </div>
                  <div class="form-item mb-2" name="jump_port"><label
                      class="text-sm text-muted-foreground mb-1">端口</label>
                    <Input size="sm" v-model="formState.jump_port" placeholder="22" />
                  </div>
                  <div class="form-item mb-2">
                    <label class="text-sm text-muted-foreground mb-1">用户名</label>
                    <Input size="sm" class="w-[280px] flex-none" v-model="formState.jump_username"
                      placeholder="跳板机用户名" />
                  </div>
                  <div class="form-item mb-2"><label class="text-sm text-muted-foreground mb-1">认证方式</label>
                    <div class="flex gap-1">
                      <button type="button"
                        :class="['px-3 py-1 rounded text-sm font-medium outline-none transition-[background,color,box-shadow]', 'focus-visible:bg-[var(--app-focus-bg)] focus-visible:text-foreground focus-visible:shadow-[var(--app-focus-shadow)]', formState.jump_auth_type === 'password' ? 'bg-primary text-primary-foreground' : 'bg-secondary text-secondary-foreground hover:bg-accent']"
                        @click="formState.jump_auth_type = 'password'">
                        <Eye :size="14" class="auth-icon inline" />
                        <span>密码认证</span>
                      </button>
                      <button type="button"
                        :class="['px-3 py-1 rounded text-sm font-medium outline-none transition-[background,color,box-shadow]', 'focus-visible:bg-[var(--app-focus-bg)] focus-visible:text-foreground focus-visible:shadow-[var(--app-focus-shadow)]', formState.jump_auth_type === 'key' ? 'bg-primary text-primary-foreground' : 'bg-secondary text-secondary-foreground hover:bg-accent']"
                        @click="formState.jump_auth_type = 'key'">
                        <FolderOpen :size="14" class="auth-icon inline" />
                        <span>私钥认证</span>
                      </button>
                    </div>
                  </div>
                  <template v-if="formState.jump_auth_type === 'password'">
                    <div class="form-item mb-2"><label class="text-sm text-muted-foreground mb-1">密码</label>
                      <Input size="sm" class="w-[280px] flex-none" v-model="formState.jump_password" type="password"
                        placeholder="留空则不修改" autocomplete="new-password" />
                    </div>
                  </template>
                  <template v-else>
                    <div class="form-item mb-2"><label class="text-sm text-muted-foreground mb-1">路径</label>
                      <Input size="sm" class="w-[280px] flex-none" v-model="formState.jump_private_key_path"
                        placeholder="选择跳板机私钥文件" readonly />
                      <Button variant="ghost" size="sm" @click="handleSelectJumpKeyFile" class="mt-1">
                        <FolderOpen :size="14" /> 浏览
                      </Button>
                    </div>
                    <div class="form-item mb-2"><label class="text-sm text-muted-foreground mb-1">口令</label>
                      <Input size="sm" class="w-[280px] flex-none" v-model="formState.jump_passphrase" type="password"
                        placeholder="留空则不修改" autocomplete="new-password" />
                    </div>
                  </template>
                  <!-- <div class="field-hint">填写后，将先连接跳板机，再通过 SSH direct-tcpip 隧道连接目标主机。</div> -->
                </div>

                <div v-if="isTelnetProtocol" class="advanced-card">
                  <div class="card-header">
                    <Server class="card-icon" />
                    <h4>Telnet 提示</h4>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="handleTestConnection" :disabled="isTestingConnection" size="sm">
          <Zap :size="14" />
          测试连接
        </Button>
        <Button variant="outline" @click="handleCancel" size="sm">取消</Button>
        <Button :disabled="confirmLoading" @click="handleOk" size="sm">
          {{ sessionData ? '保存修改' : '创建会话' }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>

<style scoped>
/* =============================================
   Tailwind @apply — 组件样式层
   ============================================= */

/* ── 协议切换 ── */
.protocol-tabs {
  @apply mb-1;
}

/* ── 表单布局 ── */
.form-item {
  @apply flex items-start min-h-8 mb-2;
}

.form-item>label {
  @apply w-[100px] shrink-0 min-h-8 pt-1.5 text-right pr-3 whitespace-nowrap text-[13px] font-normal text-[var(--app-text-muted)];
}

.form-item>input,
.form-item>select,
.form-item>textarea,
.form-item>.form-item-control {
  @apply flex-1 min-h-8;
}

/* ── 文本域 ── */
.session-textarea {
  @apply px-2.5 text-xs rounded-md outline-none transition-colors duration-200;
  background: var(--app-input-bg) !important;
  border: 1px solid var(--app-border-shadow) !important;
  color: var(--app-text) !important;
}

.session-textarea:focus {
  border-color: var(--color-primary) !important;
  box-shadow: 0 0 0 1px rgba(var(--primary), 0.28) !important;
}

.session-textarea::placeholder {
  @apply text-[var(--app-text-muted)];
}

.session-textarea {
  @apply resize-y min-h-[100px] w-full;
}

/* ── 辅助文字 ── */
.field-hint {
  @apply mt-0.5 text-[11px] leading-[1.4] text-[var(--app-text-muted)];
}

.compact-note {
  @apply ml-[104px];
}

/* ── 认证图标 ── */
.auth-icon {
  @apply mr-1 align-middle;
}

/* ── 配置标签侧边栏 ── */
.config-tabs {
  @apply min-h-0 max-h-[55vh];
}

.config-tab-sidebar {
  @apply pt-1;
}

/* ── 紧凑表单网格 ── */
.compact-form-grid {
  @apply flex flex-col gap-[0.5px];
}

/* ── 高级功能区（卡片外观已剥离，保持透明） ── */
.advanced-section {
  @apply flex flex-col gap-3;
}

.advanced-card {
  @apply border-0 rounded-none p-0 mt-0 bg-transparent;
}

.card-header {
  @apply hidden;
}

.card-header h4 {
  @apply m-0 text-xs text-[var(--app-text)] font-semibold;
}

.card-icon {
  @apply text-xs text-[var(--app-text-muted)];
}

/* ── 代理配置缩进 ── */
.proxy-config {
  @apply mt-2 pl-3 border-l-[3px] border-[var(--app-border-shadow)];
}

/* ── 表单网格（单列） ── */
.form-grid {
  @apply grid grid-cols-1 gap-0 mb-0;
}

.grid-col-1 {
  @apply col-span-1;
}

.grid-col-2 {
  @apply col-span-1;
}

.grid-col-1>label {
  @apply basis-24;
}

/* ── 区块标题（主标题隐藏，由模板内联渲染） ── */
.section-header {
  @apply hidden;
}

.section-header-spaced {
  @apply mt-3;
}

.section-title-row {
  @apply inline-flex items-center gap-2;
}

.section-title-row h3 {
  @apply m-0 text-[13px] font-semibold text-[var(--app-text)];
}

.section-header h3 {
  @apply m-0 text-[13px] font-semibold text-[var(--app-text)];
}

.section-header p {
  @apply m-0 text-[13px] text-[var(--app-text-muted)];
}

.section-tip-icon {
  @apply text-xs text-[var(--app-text-muted)] cursor-help;
}

/* =============================================
   响应式
   ============================================= */
@media (max-width: 900px) {
  .session-modal-container {
    @apply max-h-[72vh];
  }

  .compact-note {
    @apply ml-0;
  }
}
</style>
