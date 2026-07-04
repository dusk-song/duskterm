import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

// Toggle to enable/disable IPC debug logging (set to true for local debugging)
const IPC_DEBUG = false;
const TAURI_UNAVAILABLE = 'TAURI_UNAVAILABLE';

export function isTauriRuntime() {
  return !!globalThis.__TAURI_INTERNALS__;
}

function createTauriUnavailableError(cmd) {
  const error = new Error(`Tauri IPC is unavailable while calling ${cmd}`);
  error.code = TAURI_UNAVAILABLE;
  return error;
}

/**
 * 统一 IPC 调用封装
 * @param {string} cmd 命令名称
 * @param {object} args 参数对象
 * @returns {Promise<any>}
 */
export async function invokeCommand(cmd, args = {}) {
  if (!isTauriRuntime()) {
    throw createTauriUnavailableError(cmd);
  }

  try {
    if (IPC_DEBUG) console.log(`[IPC] Call: ${cmd}`, args);
    const result = await invoke(cmd, args);
    if (IPC_DEBUG) console.log(`[IPC] Result (${cmd}):`, result);
    return result;
  } catch (error) {
    console.error(`[IPC] Error (${cmd}):`, error);
    // 这里可以接入统一的错误提示 UI，比如 Toast
    throw error;
  }
}

/**
 * 监听 Rust 事件
 * @param {string} event 事件名称
 * @param {function} callback 回调函数
 * @returns {Promise<function>} 取消监听的函数
 */
export async function listenEvent(event, callback) {
  if (!isTauriRuntime()) {
    return async () => {};
  }

  return await listen(event, (eventObj) => {
    if (IPC_DEBUG) console.log(`[Event] ${event}`, eventObj.payload);
    if (callback) callback(eventObj.payload);
  });
}
