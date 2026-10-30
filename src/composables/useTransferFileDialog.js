import { ref } from 'vue';

// Native modal dialogs disable their owner window. Never open overlapping pickers
// from repeated clicks, or from separate file-manager panels.
export const transferFileDialogOpen = ref(false);

export async function withTransferFileDialog(select) {
  if (transferFileDialogOpen.value) return null;
  transferFileDialogOpen.value = true;
  try {
    return await select();
  } finally {
    transferFileDialogOpen.value = false;
  }
}
