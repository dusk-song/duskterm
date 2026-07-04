import { ref } from 'vue';

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

/**
 * 命令式确认弹窗 —— 替代 antd confirm
 * @example await confirm({ title: '删除', content: '确定？', danger: true })
 */
export function confirm(options = {}) {
  return new Promise((resolve, reject) => {
    state.value = {
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
    };
  });
}

export function useConfirm() {
  const onOk = () => {
    const { resolve, onOk } = state.value;
    state.value.visible = false;
    if (onOk) onOk();
    resolve?.();
  };

  const onCancel = () => {
    const { reject, onCancel } = state.value;
    state.value.visible = false;
    if (onCancel) onCancel();
    reject?.();
  };

  return { confirmState: state, onOk, onCancel };
}
