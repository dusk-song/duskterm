import { invokeCommand } from '@/utils/ipc';

let capabilityRequest;

export function getNativeFileDragCapabilities() {
  capabilityRequest ||= invokeCommand('native_drag_capabilities').catch((error) => {
    capabilityRequest = null;
    throw error;
  });
  return capabilityRequest;
}

export function startNativeLocalFileDrag(paths) {
  const localPaths = (Array.isArray(paths) ? paths : [paths]).filter(Boolean);
  if (!localPaths.length) {
    return Promise.reject(new Error('没有可拖动的本地文件'));
  }
  return invokeCommand('start_native_local_file_drag', { paths: localPaths });
}

export function startNativeSftpFileDrag({
  sessionId,
  remotePath,
  fileName,
  size = 0,
  reqId
}) {
  return invokeCommand('start_native_sftp_file_drag', {
    sessionId,
    remotePath,
    fileName,
    size: Number(size || 0),
    reqId
  });
}
