<script>
import { defineComponent, h } from 'vue';
import { setCustomComponents, disableMermaid, disableKatex, disableD2, disableInfographic } from 'markstream-vue';

// Keep model output passive. Scoped overrides are also the extension point for
// future agent/terminal nodes; they do not affect other Markdown consumers.
const escapedHtml = defineComponent({
  inheritAttrs: false, props: ['node'],
  setup: props => () => h('span', { class: 'escaped-html' }, props.node.raw ?? props.node.content ?? ''),
});
setCustomComponents('duskterm-ai', {
  html_block: escapedHtml, html_inline: escapedHtml,
  image: defineComponent({
    inheritAttrs: false, props: ['node'],
    setup: props => () => h('span', { class: 'image-placeholder' }, `[图片未加载：${props.node.alt || '图片'}]`),
  }),
});
disableMermaid(); disableKatex(); disableD2(); disableInfographic();
</script>

<script setup>
import { computed, onMounted } from 'vue';
import { initializeAiHighlight } from '@/utils/aiHighlight';
import MarkdownRender from 'markstream-vue';
import 'markstream-vue/index.css';
import { openUrl } from '@tauri-apps/plugin-opener';
import { useTheme } from '@/composables/useTheme';
import { toast } from '@/composables/useToast';

const props = defineProps({ content: { type: String, default: '' }, streaming: Boolean, dark: { type: Boolean, default: undefined } });
const emit = defineEmits(['copy-code']);
onMounted(initializeAiHighlight);
const { isDark } = useTheme();
const darkMode = computed(() => props.dark ?? isDark.value);
const codeProps = {
  theme: { light: 'vitesse-light', dark: 'vitesse-dark' }, minWidth: 0, maxWidth: '100%',
  showCopyButton: true, showLineNumbers: false, showPreviewButton: false,
  isShowPreview: false, showExpandButton: false, showCollapseButton: false,
  showFontSizeButtons: false, enableFontSizeControl: false, showTooltips: false,
  async clipboardWriter(code) {
    try { await navigator.clipboard.writeText(code); toast.success('代码已复制'); emit('copy-code', { code }); }
    catch (error) { toast.error('复制失败，请手动选择文本'); throw error; }
  },
};
const codeOptions = { fontSize: 12, lineHeight: 20, maxHeight: 480, overflow: 'scroll', disableLineNumbers: true, preferredHighlighter: 'shiki-js' };
async function openLink(event) {
  const link = event.target.closest('a');
  if (!link) return;
  event.preventDefault();
  try {
    const url = new URL(link.getAttribute('href'));
    if (['http:', 'https:'].includes(url.protocol)) await openUrl(url.href);
  } catch { toast.error('链接无法打开'); }
}
</script>

<template>
  <div class="ai-markdown" @click="openLink">
    <MarkdownRender :content="content" :final="!streaming" mode="chat" custom-id="duskterm-ai"
      html-policy="escape" :is-dark="darkMode" :show-tooltips="false" :typewriter="false"
      :smooth-streaming="streaming ? 'auto' : false" :batch-rendering="true" node-virtual="auto"
      :initial-render-batch-size="16" :render-batch-size="32" :render-batch-budget-ms="6"
      :max-live-nodes="160" :live-node-buffer="30" :parse-coalesce-ms="32"
      :defer-nodes-until-visible="true" :viewport-priority="true" :code-block-stream="true" :code-block-props="codeProps" :code-block-options="codeOptions" />
  </div>
</template>

<style scoped>
.ai-markdown{min-width:0;line-height:1.7;overflow-wrap:anywhere}
.ai-markdown :deep(.markstream-vue) {
  /* Bridge renderer colors to the active application theme. */
  color:inherit;
  font-size:inherit;
  --ms-background:var(--popover);
  --ms-foreground:var(--foreground);
  --ms-primary:var(--primary);
  --ms-primary-foreground:var(--primary-foreground);
  --ms-secondary:var(--secondary);
  --ms-muted:var(--muted);
  --ms-muted-foreground:var(--muted-foreground);
  --ms-border:var(--border);
  --ms-accent:var(--accent);
  --ms-accent-foreground:var(--accent-foreground);
  --ms-info:var(--primary);
  --table-border:var(--app-panel-border);
  --table-header-bg:var(--app-input-bg);
  --inline-code-border:var(--app-panel-border);
  --code-bg:var(--app-bg-dialog);
  --code-fg:var(--app-text);
  --code-border:var(--app-panel-border);
  --code-header-bg:var(--app-input-bg);
  --code-action-fg:var(--app-text-muted);
  --code-action-hover-bg:var(--app-btn-hover);
  --code-action-hover-fg:var(--app-text);
  --inline-code-bg:var(--app-btn-hover);
  --inline-code-fg:var(--app-text);
  --link-color:var(--color-primary);
  --blockquote-border:var(--app-border-light);
  --blockquote-fg:var(--app-text-muted)
}
.ai-markdown :deep(pre){max-width:100%;overflow:auto}.ai-markdown :deep(code){font-size:12px}
.ai-markdown :deep(.image-placeholder){color:var(--app-text-muted)}.ai-markdown :deep(table){max-width:100%;overflow:auto}
</style>
