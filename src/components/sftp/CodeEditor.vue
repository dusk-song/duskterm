<script setup>
import { useTheme } from '@/composables/useTheme';
import { TooltipHint } from '@/components/ui/tooltip';
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
  apache_conf: () => import('ace-builds/src-noconflict/mode-apache_conf'),
  astro: () => import('ace-builds/src-noconflict/mode-astro'),
  csharp: () => import('ace-builds/src-noconflict/mode-csharp'),
  csv: () => import('ace-builds/src-noconflict/mode-csv'),
  dart: () => import('ace-builds/src-noconflict/mode-dart'),
  diff: () => import('ace-builds/src-noconflict/mode-diff'),
  dockerfile: () => import('ace-builds/src-noconflict/mode-dockerfile'),
  ejs: () => import('ace-builds/src-noconflict/mode-ejs'),
  elixir: () => import('ace-builds/src-noconflict/mode-elixir'),
  erlang: () => import('ace-builds/src-noconflict/mode-erlang'),
  fsharp: () => import('ace-builds/src-noconflict/mode-fsharp'),
  gitignore: () => import('ace-builds/src-noconflict/mode-gitignore'),
  groovy: () => import('ace-builds/src-noconflict/mode-groovy'),
  handlebars: () => import('ace-builds/src-noconflict/mode-handlebars'),
  haskell: () => import('ace-builds/src-noconflict/mode-haskell'),
  ini: () => import('ace-builds/src-noconflict/mode-ini'),
  jsx: () => import('ace-builds/src-noconflict/mode-jsx'),
  lua: () => import('ace-builds/src-noconflict/mode-lua'),
  makefile: () => import('ace-builds/src-noconflict/mode-makefile'),
  nginx: () => import('ace-builds/src-noconflict/mode-nginx'),
  nix: () => import('ace-builds/src-noconflict/mode-nix'),
  perl: () => import('ace-builds/src-noconflict/mode-perl'),
  prisma: () => import('ace-builds/src-noconflict/mode-prisma'),
  properties: () => import('ace-builds/src-noconflict/mode-properties'),
  protobuf: () => import('ace-builds/src-noconflict/mode-protobuf'),
  r: () => import('ace-builds/src-noconflict/mode-r'),
  scala: () => import('ace-builds/src-noconflict/mode-scala'),
  svg: () => import('ace-builds/src-noconflict/mode-svg'),
  terraform: () => import('ace-builds/src-noconflict/mode-terraform'),
  tsv: () => import('ace-builds/src-noconflict/mode-tsv'),
  tsx: () => import('ace-builds/src-noconflict/mode-tsx'),
  twig: () => import('ace-builds/src-noconflict/mode-twig'),
  vbscript: () => import('ace-builds/src-noconflict/mode-vbscript'),
  vue: () => import('ace-builds/src-noconflict/mode-vue'),
  zig: () => import('ace-builds/src-noconflict/mode-zig'),
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
  go: 'ace/mode/golang',
  markdown: 'ace/mode/markdown',
  ruby: 'ace/mode/ruby',
  graphql: 'ace/mode/graphqlschema',
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
  readonly: { type: Boolean, default: false }
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
let _findDebounceTimer = null;
let _findCountTimer = null;
let _findCountScheduleKind = null;
let _findCountGeneration = 0;
let _resizeFrame = null;
let _cursorFrame = null;
let _lastCursorLine = 0;
let _lastCursorColumn = 0;
let _externalValue = props.modelValue || '';
const FIND_INPUT_DEBOUNCE_MS = 150;
const FIND_DOCUMENT_IDLE_MS = 250;
const FIND_COUNT_SLICE_BUDGET_MS = 4;
const FIND_COUNT_MAX_ROWS_PER_SLICE = 512;
const FIND_REGEX_MAX_LINE_LENGTH = 250_000;

function emitCursorPosition() {
  if (!editorInstance || _cursorFrame !== null) return;
  _cursorFrame = requestAnimationFrame(() => {
    _cursorFrame = null;
    if (!editorInstance) return;
    const position = editorInstance.getCursorPosition();
    const line = Number(position?.row || 0) + 1;
    const column = Number(position?.column || 0) + 1;
    if (line === _lastCursorLine && column === _lastCursorColumn) return;
    _lastCursorLine = line;
    _lastCursorColumn = column;
    emit('cursor-change', {
      line,
      column
    });
  });
}

function scheduleEditorResize() {
  if (!editorInstance || _resizeFrame !== null) return;
  _resizeFrame = requestAnimationFrame(() => {
    _resizeFrame = null;
    editorInstance?.resize();
  });
}

function setDirty(nextDirty) {
  if (_dirty === nextDirty) return;
  _dirty = nextDirty;
  emit('dirty-change', nextDirty);
}

function applyPerformanceOptions() {
  if (!editorInstance) return;
  editorInstance.setOptions({
    useWorker: false,
    highlightActiveLine: true,
    highlightSelectedWord: true,
    displayIndentGuides: true,
    showFoldWidgets: true,
    animatedScroll: false,
    enableBasicAutocompletion: false,
    enableLiveAutocompletion: false,
    enableSnippets: false,
    showPrintMargin: false,
    scrollPastEnd: 0,
    fadeFoldWidgets: false,
    behavioursEnabled: true,
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

function cancelFindCount() {
  _findCountGeneration += 1;
  if (_findCountTimer !== null) {
    if (_findCountScheduleKind === 'idle') {
      window.cancelIdleCallback(_findCountTimer);
    } else {
      clearTimeout(_findCountTimer);
    }
    _findCountTimer = null;
    _findCountScheduleKind = null;
  }
}

function scheduleFindCountSlice(callback) {
  if (typeof window.requestIdleCallback === 'function') {
    _findCountScheduleKind = 'idle';
    _findCountTimer = window.requestIdleCallback((deadline) => {
      _findCountTimer = null;
      _findCountScheduleKind = null;
      callback(deadline);
    }, { timeout: 120 });
    return;
  }
  _findCountScheduleKind = 'timeout';
  _findCountTimer = setTimeout(() => {
    _findCountTimer = null;
    _findCountScheduleKind = null;
    callback(null);
  }, 0);
}

function createFindPattern() {
  const needle = findQuery.value;
  const source = findOptions.regExp ? needle : escapeRegExp(needle);
  const boundedSource = findOptions.wholeWord ? `\\b(?:${source})\\b` : source;
  const flags = findOptions.caseSensitive ? 'g' : 'gi';
  return new RegExp(boundedSource, flags);
}

function startExactFindCount() {
  cancelFindCount();
  if (!editorInstance || !findQuery.value) {
    findCountLabel.value = '0 个结果';
    return;
  }

  let pattern;
  try {
    pattern = createFindPattern();
  } catch {
    findCountLabel.value = '表达式无效';
    return;
  }

  const generation = _findCountGeneration;
  const session = editorInstance.getSession();
  const totalRows = session.getLength();
  let row = 0;
  let count = 0;
  findCountLabel.value = '统计中…';

  const countSlice = (idleDeadline) => {
    if (generation !== _findCountGeneration || !editorInstance) return;
    const startedAt = performance.now();
    let rowsProcessed = 0;
    const hasTimeRemaining = () => {
      const withinFallbackBudget = performance.now() - startedAt < FIND_COUNT_SLICE_BUDGET_MS;
      if (!idleDeadline || idleDeadline.didTimeout) return withinFallbackBudget;
      return idleDeadline.timeRemaining() > 1;
    };
    while (
      row < totalRows
      && rowsProcessed < FIND_COUNT_MAX_ROWS_PER_SLICE
      && (rowsProcessed === 0 || hasTimeRemaining())
    ) {
      const line = session.getLine(row) || '';
      if (findOptions.regExp && line.length > FIND_REGEX_MAX_LINE_LENGTH) {
        findCountLabel.value = '结果数未统计';
        return;
      }
      pattern.lastIndex = 0;
      let match = pattern.exec(line);
      while (match) {
        count += 1;
        if (match[0] === '') pattern.lastIndex += 1;
        match = pattern.exec(line);
      }
      row += 1;
      rowsProcessed += 1;
    }

    if (row < totalRows) {
      scheduleFindCountSlice(countSlice);
      return;
    }
    findCountLabel.value = count > 0 ? `${count} 个结果` : '无结果';
  };
  scheduleFindCountSlice(countSlice);
}

function performFind(backwards = false, navigate = true) {
  if (!editorInstance || !findQuery.value) {
    findCountLabel.value = '0 个结果';
    return;
  }
  try {
    createFindPattern();
    if (navigate) editorInstance.find(findQuery.value, searchOptions(backwards));
    startExactFindCount();
  } catch {
    findCountLabel.value = '表达式无效';
  }
}

function scheduleFind(options = {}) {
  const { backwards = false, navigate = true, delay = FIND_INPUT_DEBOUNCE_MS } = options;
  if (_findDebounceTimer) clearTimeout(_findDebounceTimer);
  cancelFindCount();
  if (!findQuery.value) {
    findCountLabel.value = '0 个结果';
    return;
  }
  try {
    createFindPattern();
    findCountLabel.value = '等待查找…';
  } catch {
    findCountLabel.value = '表达式无效';
    return;
  }
  _findDebounceTimer = setTimeout(() => {
    _findDebounceTimer = null;
    performFind(backwards, navigate);
  }, delay);
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
    scheduleEditorResize();
    if (replaceVisible.value && options.replace) {
      replaceInput.value?.focus();
      replaceInput.value?.select();
    } else {
      findInput.value?.focus();
      findInput.value?.select();
    }
    scheduleFind({ navigate: true, delay: 0 });
  });
}

function closeFindBar() {
  if (_findDebounceTimer) clearTimeout(_findDebounceTimer);
  _findDebounceTimer = null;
  cancelFindCount();
  findVisible.value = false;
  replaceVisible.value = false;
  nextTick(() => {
    scheduleEditorResize();
    editorInstance?.focus();
  });
}

function findNextMatch() {
  if (!editorInstance || !findQuery.value) return;
  try {
    editorInstance.findNext();
  } catch {
    findCountLabel.value = '表达式无效';
  }
}

function findPreviousMatch() {
  if (!editorInstance || !findQuery.value) return;
  try {
    editorInstance.findPrevious();
  } catch {
    findCountLabel.value = '表达式无效';
  }
}

function replaceCurrentMatch() {
  if (props.readonly || !editorInstance || !findQuery.value) return;
  try {
    editorInstance.replace(replaceText.value);
    scheduleFind({ navigate: false, delay: 0 });
  } catch {
    findCountLabel.value = '表达式无效';
  }
}

function replaceAllMatches() {
  if (props.readonly || !editorInstance || !findQuery.value) return;
  try {
    editorInstance.replaceAll(replaceText.value);
    scheduleFind({ navigate: false, delay: 0 });
  } catch {
    findCountLabel.value = '表达式无效';
  }
}

function toggleFindOption(option) {
  findOptions[option] = !findOptions[option];
  scheduleFind({ navigate: true, delay: 0 });
}

function toggleReplaceBar() {
  replaceVisible.value = !replaceVisible.value;
  nextTick(() => {
    scheduleEditorResize();
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
  const nextValue = value || '';
  _externalValue = nextValue;
  _suppressChange = true;
  editorInstance.setValue(nextValue, -1);
  _suppressChange = false;
  if (options.clean) {
    editorInstance.getSession().getUndoManager()?.markClean?.();
    setDirty(false);
  }
  emitCursorPosition();
}

function acknowledgeValue(value = '') {
  _externalValue = value || '';
}

function markClean(value, options = {}) {
  if (typeof value === 'string') {
    if (!options.current && editorInstance && editorInstance.getValue() !== value) {
      setValue(value, { clean: true });
      return;
    }
    acknowledgeValue(value);
  }
  editorInstance?.getSession().getUndoManager()?.markClean?.();
  setDirty(false);
}

function focus() {
  editorInstance?.focus();
}

function resize() {
  scheduleEditorResize();
}

defineExpose({
  getValue,
  setValue,
  acknowledgeValue,
  markClean,
  focus,
  resize
});

onMounted(async () => {
  if (!editorContainer.value) return;

  const mode = langToAceMode(props.language);
  await ensureModeLoaded(mode);
  if (!editorContainer.value) return;

  _externalValue = props.modelValue || '';
  editorInstance = ace.edit(editorContainer.value, {
    value: _externalValue,
    mode,
    theme: aceTheme(),
    readOnly: props.readonly,
    fontSize: 14,
    fontFamily: "'Maple Mono', 'Microsoft YaHei Mono', 'SimSun', monospace",
    tabSize: 2,
    useSoftTabs: true,
    wrap: false,
  });

  applyPerformanceOptions();

  _resizeObs = new ResizeObserver(scheduleEditorResize);
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
    const undoManager = editorInstance?.getSession().getUndoManager();
    setDirty(!(undoManager?.isClean?.() ?? false));
    if (findVisible.value && findQuery.value) {
      scheduleFind({ navigate: false, delay: FIND_DOCUMENT_IDLE_MS });
    }
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
  if (_findDebounceTimer) clearTimeout(_findDebounceTimer);
  _findDebounceTimer = null;
  cancelFindCount();
  _stopThemeWatch?.();
  _stopThemeWatch = null;
  _resizeObs?.disconnect();
  _resizeObs = null;
  if (_resizeFrame !== null) cancelAnimationFrame(_resizeFrame);
  _resizeFrame = null;
  if (_cursorFrame !== null) cancelAnimationFrame(_cursorFrame);
  _cursorFrame = null;
  editorInstance?.destroy();
  editorInstance?.container?.remove();
  editorInstance = null;
});

watch(() => props.modelValue, (value) => {
  if (!editorInstance) return;
  const nextValue = value || '';
  if (_externalValue === nextValue) return;
  setValue(nextValue, { clean: true });
});

watch(() => props.language, async (language) => {
  if (!editorInstance) return;
  const mode = langToAceMode(language);
  await ensureModeLoaded(mode);
  editorInstance.getSession().setMode(mode);
});

watch(() => props.readonly, (readonly) => {
  editorInstance?.setReadOnly(readonly);
});

</script>

<template>
  <div class="ace-editor-frame">
    <div v-if="findVisible" class="editor-find-bar" @keydown.stop>
      <div class="editor-find-row editor-find-main-row">
        <div class="find-input-shell search-input-shell">
          <input ref="findInput" v-model="findQuery" class="find-input" type="text" placeholder="查找"
            @input="() => scheduleFind()" @keydown.enter.prevent="handleFindEnter"
            @keydown.esc.prevent="closeFindBar" />
          <span class="find-count">{{ findCountLabel }}</span>
        </div>
        <TooltipHint text="区分大小写">
          <button type="button" class="find-button find-icon-button option-button"
            :class="{ active: findOptions.caseSensitive }" aria-label="区分大小写"
            @click="toggleFindOption('caseSensitive')">
            <CaseSensitive :size="15" stroke-width="1.9" />
          </button>
        </TooltipHint>
        <TooltipHint text="全词匹配">
          <button type="button" class="find-button find-icon-button option-button"
            :class="{ active: findOptions.wholeWord }" aria-label="全词匹配"
            @click="toggleFindOption('wholeWord')">
            <WholeWord :size="15" stroke-width="1.9" />
          </button>
        </TooltipHint>
        <TooltipHint text="正则表达式">
          <button type="button" class="find-button find-icon-button option-button"
            :class="{ active: findOptions.regExp }" aria-label="正则表达式"
            @click="toggleFindOption('regExp')">
            <Regex :size="15" stroke-width="1.9" />
          </button>
        </TooltipHint>
        <span class="find-divider"></span>
        <TooltipHint text="上一个">
          <button type="button" class="find-button find-icon-button" aria-label="上一个"
            @click="findPreviousMatch">
            <ChevronUp :size="15" stroke-width="1.9" />
          </button>
        </TooltipHint>
        <TooltipHint text="下一个">
          <button type="button" class="find-button find-icon-button" aria-label="下一个"
            @click="findNextMatch">
            <ChevronDown :size="15" stroke-width="1.9" />
          </button>
        </TooltipHint>
        <TooltipHint text="显示替换">
          <button type="button" class="find-button find-icon-button" :class="{ active: replaceVisible }"
            aria-label="显示替换" @click="toggleReplaceBar">
            <Replace :size="15" stroke-width="1.9" />
          </button>
        </TooltipHint>
        <TooltipHint text="关闭查找">
          <button type="button" class="find-close find-icon-button" aria-label="关闭查找"
            @click="closeFindBar">
            <X :size="15" stroke-width="1.9" />
          </button>
        </TooltipHint>
      </div>
      <div v-if="replaceVisible" class="editor-find-row editor-find-replace-row">
        <div class="find-input-shell replace-input-shell">
          <input ref="replaceInput" v-model="replaceText" class="find-input" type="text" placeholder="替换为"
            :disabled="readonly" @keydown.enter.prevent="replaceCurrentMatch" @keydown.esc.prevent="closeFindBar" />
        </div>
        <TooltipHint text="替换当前">
          <button type="button" class="find-button find-icon-button" :disabled="readonly" aria-label="替换当前"
            @click="replaceCurrentMatch">
            <Replace :size="15" stroke-width="1.9" />
          </button>
        </TooltipHint>
        <TooltipHint text="全部替换">
          <button type="button" class="find-button find-icon-button" :disabled="readonly" aria-label="全部替换"
            @click="replaceAllMatches">
            <ReplaceAll :size="15" stroke-width="1.9" />
          </button>
        </TooltipHint>
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
  font-family: var(--app-font-family);
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
  font-family: var(--app-font-family);
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
  font-family: var(--app-font-family);
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
