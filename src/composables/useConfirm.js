import { nextTick, ref } from 'vue';

const state = ref({
  visible: false,
  title: '',
  content: '',
  okText: '确认',
  cancelText: '取消',
  danger: false,
  onOk: null,
  onCancel: null,
  resolve: null,
  reject: null,
  zIndex: 2200,
});
const pending = [];
let closing = false;

function showNext() {
  if (!closing && !state.value.visible && pending.length) state.value = pending.shift();
}

/**
 * 命令式确认弹窗 —— 替代 antd confirm
 * @example await confirm({ title: '删除', content: '确定？', danger: true })
 */
export function confirm(options = {}) {
  return new Promise((resolve, reject) => {
    pending.push({
      visible: true,
      title: options.title || '提示',
      content: options.content || '',
      okText: options.okText || '确认',
      cancelText: options.cancelText || '取消',
      danger: options.danger || options.okButtonProps?.danger || false,
      onOk: options.onOk || null,
      onCancel: options.onCancel || null,
      zIndex: options.zIndex ?? 2200,
      resolve,
      reject,
    });
    showNext();
  });
}

export function useConfirm() {
  const finish = (accepted) => {
    if (!state.value.visible) return;
    const current = state.value;
    closing = true;
    state.value.visible = false;
    try {
      if (accepted) {
        current.onOk?.();
        current.resolve?.();
      } else {
        current.onCancel?.();
        current.reject?.();
      }
    } catch (error) {
      current.reject?.(error);
    } finally {
      // Let Reka release the previous modal's focus/pointer guards before the next one.
      nextTick(() => { closing = false; showNext(); });
    }
  };
  const onOk = () => finish(true);
  const onCancel = () => finish(false);

  return { confirmState: state, onOk, onCancel };
}
