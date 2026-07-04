<script setup>
import { useTheme } from '@/composables/useTheme';
import ace from 'ace-builds';
import 'ace-builds/src-noconflict/theme-github';
import 'ace-builds/src-noconflict/theme-tomorrow_night_bright';
import {
  CaseSensitive,
  ChevronDown,
  ChevronUp,
  Regex,
  Replace,
  ReplaceAll,
  WholeWord,
  X
} from '@lucide/vue';
import { nextTick, onMounted, onUnmounted, reactive, ref, watch } from 'vue';

const loadedModes = new Set(['ace/mode/text']);
const MODE_IMPORTS = {
  javascript: () => import('ace-builds/src-noconflict/mode-javascript'),
  typescript: () => import('ace-builds/src-noconflict/mode-typescript'),
  json: () => import('ace-builds/src-noconflict/mode-json'),
  html: () => import('ace-builds/src-noconflict/mode-html'),
  css: () => import('ace-builds/src-noconflict/mode-css'),
  python: () => import('ace-builds/src-noconflict/mode-python'),
  rust: () => import('ace-builds/src-noconflict/mode-rust'),
  sh: () => import('ace-builds/src-noconflict/mode-sh'),
  yaml: () => import('ace-builds/src-noconflict/mode-yaml'),
  toml: () => import('ace-builds/src-noconflict/mode-toml'),
  xml: () => import('ace-builds/src-noconflict/mode-xml'),
  sql: () => import('ace-builds/src-noconflict/mode-sql'),
  markdown: () => import('ace-builds/src-noconflict/mode-markdown'),
  c_cpp: () => import('ace-builds/src-noconflict/mode-c_cpp'),
  java: () => import('ace-builds/src-noconflict/mode-java'),
  golang: () => import('ace-builds/src-noconflict/mode-golang'),
  php: () => import('ace-builds/src-noconflict/mode-php'),
  ruby: () => import('ace-builds/src-noconflict/mode-ruby'),
  swift: () => import('ace-builds/src-noconflict/mode-swift'),
  kotlin: () => import('ace-builds/src-noconflict/mode-kotlin'),
  batchfile: () => import('ace-builds/src-noconflict/mode-batchfile'),
  powershell: () => import('ace-builds/src-noconflict/mode-powershell'),
  graphqlschema: () => import('ace-builds/src-noconflict/mode-graphqlschema'),
  less: () => import('ace-builds/src-noconflict/mode-less'),
  scss: () => import('ace-builds/src-noconflict/mode-scss'),
};

async function ensureModeLoaded(aceMode) {
  const name = aceMode.replace('ace/mode/', '');
  if (loadedModes.has(aceMode) || name === 'text') return;
  const loader = MODE_IMPORTS[name];
  if (loader) {
    await loader();
    loadedModes.add(aceMode);
  }
}

const LANG_TO_ACE_MODE = {
  plaintext: 'ace/mode/text',
  shell: 'ace/mode/sh',
  bat: 'ace/mode/batchfile',
  powershell: 'ace/mode/powershell',
  cpp: 'ace/mode/c_cpp',
  c: 'ace/mode/c_cpp',
  h: 'ace/mode/c_cpp',
  hpp: 'ace/mode/c_cpp',
  hxx: 'ace/mode/c_cpp',
  markdown: 'ace/mode/markdown',
  ruby: 'ace/mode/ruby',
  graphql: 'ace/mode/graphqlschema',
  ini: 'ace/mode/toml',
  less: 'ace/mode/less',
  scss: 'ace/mode/scss',
};

function langToAceMode(lang) {
  if (!lang) return 'ace/mode/text';
  return LANG_TO_ACE_MODE[lang] || `ace/mode/${lang}`;
}

const props = defineProps({
  modelValue: String,
  language: { type: String, default: 'plaintext' },
  readonly: { type: Boolean, default: false },
  largeFile: { type: Boolean, default: false }
});

const emit = defineEmits(['ready', 'dirty-change', 'cursor-change', 'save']);
const { isDark } = useTheme();
const aceTheme = () => isDark.value ? 'ace/theme/tomorrow_night_bright' : 'ace/theme/github';
const editorContainer = ref(null);
const findInput = ref(null);
const replaceInput = ref(null);
const findVisible = ref(false);
const replaceVisible = ref(false);
const findQuery = ref('');
const replaceText = ref('');
const findCountLabel = ref('0 个结果');
const findOptions = reactive({
  caseSensitive: false,
  wholeWord: false,
  regExp: false
});
let editorInstance = null;
let _resizeObs = null;
let _stopThemeWatch = null;
let _suppressChange = false;
let _dirty = false;

function emitCursorPosition() {
  if (!editorInstance) return;
  const position = editorInstance.getCursorPosition();
  emit('cursor-change', {
    line: Number(position?.row || 0) + 1,
    column: Number(position?.column || 0) + 1
  });
}

function setDirty(nextDirty) {
  if (_dirty === nextDirty) return;
  _dirty = nextDirty;
  emit('dirty-change', nextDirty);
}

function applyPerformanceOptions() {
  if (!editorInstance) return;
  const richRendering = !props.largeFile;
  editorInstance.setOptions({
    useWorker: false,
    highlightActiveLine: !props.largeFile,
    highlightSelectedWord: !props.largeFile,
    displayIndentGuides: !props.largeFile,
    showFoldWidgets: !props.largeFile,
    animatedScroll: false,
    enableBasicAutocompletion: false,
    enableLiveAutocompletion: false,
    enableSnippets: false,
    showPrintMargin: false,
    scrollPastEnd: 0,
    fadeFoldWidgets: false,
    behavioursEnabled: richRendering,
  });
  editorInstance.renderer?.setShowGutter(true);
}

function escapeRegExp(value) {
  return String(value).replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function searchOptions(backwards = false) {
  return {
    backwards,
    wrap: true,
    caseSensitive: findOptions.caseSensitive,
    wholeWord: findOptions.wholeWord,
    regExp: findOptions.regExp,
    skipCurrent: false
  };
}

function updateFindCount() {
  const needle = findQuery.value;
  if (!editorInstance || !needle) {
    findCountLabel.value = '0 个结果';
    return;
  }

  const content = editorInstance.getValue();
  try {
    const source = findOptions.regExp ? needle : escapeRegExp(needle);
    const boundedSource = findOptions.wholeWord ? `\\b(?:${source})\\b` : source;
    const flags = findOptions.caseSensitive ? 'g' : 'gi';
    const pattern = new RegExp(boundedSource, flags);
    let count = 0;
    let match = pattern.exec(content);
    while (match) {
      count += 1;
      if (match[0] === '') pattern.lastIndex += 1;
      match = pattern.exec(content);
    }
    findCountLabel.value = count > 0 ? `${count} 个结果` : '无结果';
  } catch {
    findCountLabel.value = '表达式无效';
  }
}

function runFind(backwards = false) {
  updateFindCount();
  if (!editorInstance || !findQuery.value || findCountLabel.value === '表达式无效') return;
  try {
    editorInstance.find(findQuery.value, searchOptions(backwards));
  } catch {
    findCountLabel.value = '表达式无效';
  }
}

function openFindBar(options = {}) {
  if (!editorInstance) return;
  findVisible.value = true;
  replaceVisible.value = !!options.replace || replaceVisible.value;
  const selectedText = editorInstance.getSelectedText?.() || '';
  if (selectedText && !selectedText.includes('\n')) {
    findQuery.value = selectedText;
  }
  nextTick(() => {
    editorInstance?.resize();
    if (replaceVisible.value && options.replace) {
      replaceInput.value?.focus();
      replaceInput.value?.select();
    } else {
      findInput.value?.focus();
      findInput.value?.select();
    }
    runFind(false);
  });
}

function closeFindBar() {
  findVisible.value = false;
  replaceVisible.value = false;
  nextTick(() => {
    editorInstance?.resize();
    editorInstance?.focus();
  });
}

function findNextMatch() {
  if (!editorInstance || !findQuery.value) return;
  try {
    editorInstance.findNext();
    updateFindCount();
  } catch {
    findCountLabel.value = '表达式无效';
  }
}

function findPreviousMatch() {
  if (!editorInstance || !findQuery.value) return;
  try {
    editorInstance.findPrevious();
    updateFindCount();
  } catch {
    findCountLabel.value = '表达式无效';
  }
}

function replaceCurrentMatch() {
  if (props.readonly || !editorInstance || !findQuery.value) return;
  try {
    editorInstance.replace(replaceText.value);
    setDirty(true);
    runFind(false);
  } catch {
    findCountLabel.value = '表达式无效';
  }
}

function replaceAllMatches() {
  if (props.readonly || !editorInstance || !findQuery.value) return;
  try {
    editorInstance.replaceAll(replaceText.value);
    setDirty(true);
    updateFindCount();
  } catch {
    findCountLabel.value = '表达式无效';
  }
}

function toggleFindOption(option) {
  findOptions[option] = !findOptions[option];
  runFind(false);
}

function toggleReplaceBar() {
  replaceVisible.value = !replaceVisible.value;
  nextTick(() => {
    editorInstance?.resize();
    if (replaceVisible.value) {
      replaceInput.value?.focus();
      replaceInput.value?.select();
    } else {
      findInput.value?.focus();
    }
  });
}

function handleFindEnter(event) {
  if (event.shiftKey) {
    findPreviousMatch();
    return;
  }
  findNextMatch();
}

function getValue() {
  return editorInstance?.getValue() ?? '';
}

function setValue(value = '', options = {}) {
  if (!editorInstance) return;
  _suppressChange = true;
  editorInstance.setValue(value || '', -1);
  _suppressChange = false;
  if (options.clean) setDirty(false);
  emitCursorPosition();
}

function markClean(value) {
  if (typeof value === 'string' && editorInstance && editorInstance.getValue() !== value) {
    setValue(value, { clean: true });
    return;
  }
  setDirty(false);
}

function focus() {
  editorInstance?.focus();
}

function resize() {
  editorInstance?.resize();
}

defineExpose({
  getValue,
  setValue,
  markClean,
  focus,
  resize
});

onMounted(async () => {
  if (!editorContainer.value) return;

  const requestedMode = langToAceMode(props.language);
  const mode = props.largeFile ? 'ace/mode/text' : requestedMode;
  await ensureModeLoaded(mode);

  editorInstance = ace.edit(editorContainer.value, {
    value: props.modelValue || '',
    mode,
    theme: aceTheme(),
    readOnly: props.readonly,
    fontSize: 14,
    fontFamily: "'Cascadia Code', 'Fira Code', 'Consolas', monospace",
    tabSize: 2,
    useSoftTabs: true,
    wrap: false,
  });

  applyPerformanceOptions();

  _resizeObs = new ResizeObserver(() => editorInstance?.resize());
  _resizeObs.observe(editorContainer.value);

  editorInstance.commands.removeCommand('gotoline');
  editorInstance.commands.removeCommand('find');
  editorInstance.commands.removeCommand('replace');
  editorInstance.commands.addCommand({
    name: 'save',
    bindKey: { win: 'Ctrl-S', mac: 'Cmd-S' },
    exec: () => emit('save')
  });
  editorInstance.commands.addCommand({
    name: 'open-find-bar',
    bindKey: { win: 'Ctrl-F', mac: 'Command-F' },
    exec: () => openFindBar()
  });
  editorInstance.commands.addCommand({
    name: 'open-replace-bar',
    bindKey: { win: 'Ctrl-H', mac: 'Command-H' },
    exec: () => openFindBar({ replace: true })
  });

  editorInstance.on('change', () => {
    if (_suppressChange) return;
    setDirty(true);
  });
  editorInstance.selection.on('changeCursor', emitCursorPosition);
  editorInstance.selection.on('changeSelection', emitCursorPosition);

  _stopThemeWatch = watch(isDark, () => {
    editorInstance?.setTheme(aceTheme());
  });

  setDirty(false);
  emitCursorPosition();
  emit('ready');
});

onUnmounted(() => {
  _stopThemeWatch?.();
  _stopThemeWatch = null;
  _resizeObs?.disconnect();
  _resizeObs = null;
  editorInstance?.destroy();
  editorInstance?.container?.remove();
  editorInstance = null;
});

watch(() => props.modelValue, (value) => {
  if (!editorInstance) return;
  if (editorInstance.getValue() !== value) {
    setValue(value || '', { clean: true });
  }
});

watch(() => props.language, async (language) => {
  if (!editorInstance) return;
  const requestedMode = langToAceMode(language);
  const mode = props.largeFile ? 'ace/mode/text' : requestedMode;
  await ensureModeLoaded(mode);
  editorInstance.getSession().setMode(mode);
});

watch(() => props.readonly, (readonly) => {
  editorInstance?.setReadOnly(readonly);
});

watch(() => props.largeFile, async () => {
  if (!editorInstance) return;
  const requestedMode = langToAceMode(props.language);
  const mode = props.largeFile ? 'ace/mode/text' : requestedMode;
  await ensureModeLoaded(mode);
  editorInstance.getSession().setMode(mode);
  applyPerformanceOptions();
});
</script>

<template>
  <div class="ace-editor-frame">
    <div v-if="findVisible" class="editor-find-bar" @keydown.stop>
      <div class="editor-find-row editor-find-main-row">
        <div class="find-input-shell search-input-shell">
          <input ref="findInput" v-model="findQuery" class="find-input" type="text" placeholder="查找"
            @input="() => runFind(false)" @keydown.enter.prevent="handleFindEnter"
            @keydown.esc.prevent="closeFindBar" />
          <span class="find-count">{{ findCountLabel }}</span>
        </div>
        <button type="button" class="find-button find-icon-button option-button"
          :class="{ active: findOptions.caseSensitive }" aria-label="区分大小写" title="区分大小写"
          @click="toggleFindOption('caseSensitive')">
          <CaseSensitive :size="15" stroke-width="1.9" />
        </button>
        <button type="button" class="find-button find-icon-button option-button"
          :class="{ active: findOptions.wholeWord }" aria-label="全词匹配" title="全词匹配"
          @click="toggleFindOption('wholeWord')">
          <WholeWord :size="15" stroke-width="1.9" />
        </button>
        <button type="button" class="find-button find-icon-button option-button" :class="{ active: findOptions.regExp }"
          aria-label="正则表达式" title="正则表达式" @click="toggleFindOption('regExp')">
          <Regex :size="15" stroke-width="1.9" />
        </button>
        <span class="find-divider"></span>
        <button type="button" class="find-button find-icon-button" aria-label="上一个" title="上一个"
          @click="findPreviousMatch">
          <ChevronUp :size="15" stroke-width="1.9" />
        </button>
        <button type="button" class="find-button find-icon-button" aria-label="下一个" title="下一个"
          @click="findNextMatch">
          <ChevronDown :size="15" stroke-width="1.9" />
        </button>
        <button type="button" class="find-button find-icon-button" :class="{ active: replaceVisible }"
          aria-label="显示替换" title="显示替换" @click="toggleReplaceBar">
          <Replace :size="15" stroke-width="1.9" />
        </button>
        <button type="button" class="find-close find-icon-button" aria-label="关闭查找" title="关闭查找"
          @click="closeFindBar">
          <X :size="15" stroke-width="1.9" />
        </button>
      </div>
      <div v-if="replaceVisible" class="editor-find-row editor-find-replace-row">
        <div class="find-input-shell replace-input-shell">
          <input ref="replaceInput" v-model="replaceText" class="find-input" type="text" placeholder="替换为"
            :disabled="readonly" @keydown.enter.prevent="replaceCurrentMatch" @keydown.esc.prevent="closeFindBar" />
        </div>
        <button type="button" class="find-button find-icon-button" :disabled="readonly" aria-label="替换当前"
          title="替换当前" @click="replaceCurrentMatch">
          <Replace :size="15" stroke-width="1.9" />
        </button>
        <button type="button" class="find-button find-icon-button" :disabled="readonly" aria-label="全部替换"
          title="全部替换" @click="replaceAllMatches">
          <ReplaceAll :size="15" stroke-width="1.9" />
        </button>
      </div>
    </div>
    <div ref="editorContainer" class="ace-container"></div>
  </div>
</template>

<style>
.sftp-editor-dialog .ace_editor {
  background: var(--app-bg-dialog, #18181a) !important;
  color: var(--app-text, #e4dfd8) !important;
  font-variant-ligatures: none;
}

.sftp-editor-dialog .ace_gutter {
  background: color-mix(in srgb, var(--app-bg-dialog, #18181a) 94%, var(--app-text, #e4dfd8)) !important;
  border-right: 1px solid var(--app-border-shadow, rgba(255,255,255,0.08)) !important;
  color: color-mix(in srgb, var(--app-text-muted, #aba296) 78%, transparent) !important;
}

.sftp-editor-dialog .ace_scroller,
.sftp-editor-dialog .ace_content {
  background: var(--app-bg-dialog, #18181a) !important;
}

.sftp-editor-dialog .ace_cursor {
  color: var(--color-primary, #c0842f) !important;
  border-left-color: var(--color-primary, #c0842f) !important;
}

.sftp-editor-dialog .ace_marker-layer .ace_selection {
  background: color-mix(in srgb, var(--color-primary, #c0842f) 26%, transparent) !important;
}

.sftp-editor-dialog .ace_gutter-active-line {
  background: color-mix(in srgb, var(--app-text, #e4dfd8) 5%, transparent) !important;
}

.sftp-editor-dialog .ace_active-line {
  background: color-mix(in srgb, var(--app-text, #e4dfd8) 4%, transparent) !important;
}

.sftp-editor-dialog .ace_scrollbar-h,
.sftp-editor-dialog .ace_scrollbar-v,
.sftp-editor-dialog .ace_scrollbar-inner {
  background: var(--app-bg-dialog, #18181a) !important;
}

.sftp-editor-dialog .ace_scrollbar-h::-webkit-scrollbar-corner,
.sftp-editor-dialog .ace_scrollbar-v::-webkit-scrollbar-corner,
.sftp-editor-dialog .ace_scroller::-webkit-scrollbar-corner,
.sftp-editor-dialog .ace_editor::-webkit-scrollbar-corner {
  background: var(--app-bg-dialog, #18181a) !important;
}

.sftp-editor-dialog .ace_scrollbar-h::-webkit-resizer,
.sftp-editor-dialog .ace_scrollbar-v::-webkit-resizer,
.sftp-editor-dialog .ace_scroller::-webkit-resizer,
.sftp-editor-dialog .ace_editor::-webkit-resizer {
  background: color-mix(in srgb, var(--app-text-muted, #aba296) 18%, var(--app-bg-dialog, #18181a)) !important;
}

.sftp-editor-dialog .editor-find-bar {
  min-height: 42px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 7px 8px;
  border-bottom: 1px solid var(--app-border-shadow, rgba(255,255,255,0.08));
  background:
    linear-gradient(180deg,
      color-mix(in srgb, var(--app-bg-dialog, #18181a) 94%, var(--app-text, #e4dfd8)),
      color-mix(in srgb, var(--app-bg-dialog, #18181a) 98%, var(--app-text, #e4dfd8)));
  color: var(--app-text, #e4dfd8);
  font-family: var(--app-font-family, 'Mona Sans', 'Segoe UI', sans-serif);
  overflow-x: auto;
  overflow-y: hidden;
}

.sftp-editor-dialog .editor-find-row {
  min-width: 860px;
  display: flex;
  align-items: center;
  gap: 7px;
}

.sftp-editor-dialog .editor-find-main-row {
  min-height: 28px;
}

.sftp-editor-dialog .editor-find-replace-row {
  min-height: 28px;
}

.sftp-editor-dialog .find-input-shell {
  flex: 1 1 auto;
  min-width: 180px;
  height: 28px;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 0 8px;
  border: 1px solid var(--app-input-border, var(--app-border-shadow, rgba(255,255,255,0.08)));
  border-radius: var(--niri-radius-sm, 5px);
  background: var(--app-input-bg, rgba(255,255,255,0.04));
}

.sftp-editor-dialog .search-input-shell {
  min-width: 280px;
}

.sftp-editor-dialog .replace-input-shell {
  min-width: 280px;
}

.sftp-editor-dialog .find-input {
  min-width: 0;
  flex: 1;
  height: 24px;
  border: 0;
  outline: 0;
  background: transparent;
  color: var(--app-text, #e4dfd8);
  font-size: 12px;
  font-family: var(--app-font-family, 'Mona Sans', 'Segoe UI', sans-serif);
}

.sftp-editor-dialog .find-input::placeholder {
  color: var(--app-text-muted, #aba296);
}

.sftp-editor-dialog .find-count {
  flex: 0 0 auto;
  color: var(--app-text-muted, #aba296);
  font-size: 11px;
  white-space: nowrap;
}

.sftp-editor-dialog .find-button,
.sftp-editor-dialog .find-close {
  flex: 0 0 auto;
  height: 28px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid transparent;
  border-radius: var(--niri-radius-sm, 5px);
  background: transparent;
  color: var(--app-text-muted, #aba296);
  font-size: 12px;
  font-family: var(--app-font-family, 'Mona Sans', 'Segoe UI', sans-serif);
  line-height: 1;
  cursor: pointer;
}

.sftp-editor-dialog .find-button {
  padding: 0 9px;
}

.sftp-editor-dialog .find-icon-button {
  width: 28px;
  padding: 0;
}

.sftp-editor-dialog .find-icon-button svg {
  width: 15px;
  height: 15px;
  fill: none;
  stroke: currentColor;
}

.sftp-editor-dialog .option-button {
  color: color-mix(in srgb, var(--app-text-muted, #aba296) 86%, transparent);
}

.sftp-editor-dialog .find-divider {
  flex: 0 0 auto;
  width: 1px;
  height: 20px;
  background: var(--app-border-shadow, rgba(255,255,255,0.08));
}

.sftp-editor-dialog .find-close {
  width: 28px;
  font-size: 17px;
}

.sftp-editor-dialog .find-button:hover,
.sftp-editor-dialog .find-close:hover {
  border-color: var(--app-border-shadow, rgba(255,255,255,0.08));
  background: var(--app-btn-hover, rgba(255,255,255,0.10));
  color: var(--app-text, #e4dfd8);
}

.sftp-editor-dialog .find-button.active {
  border-color: color-mix(in srgb, var(--color-primary, #c0842f) 45%, transparent);
  background: color-mix(in srgb, var(--color-primary, #c0842f) 14%, transparent);
  color: var(--app-text, #e4dfd8);
}

.sftp-editor-dialog .find-button:disabled,
.sftp-editor-dialog .find-input:disabled {
  cursor: not-allowed;
  opacity: 0.45;
}
</style>

<style scoped>
.ace-editor-frame {
  width: 100%;
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.ace-container {
  flex: 1 1 auto;
  width: 100%;
  min-height: 0;
}
</style>
