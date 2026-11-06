// Imported text uses the existing composer/message contract, not an attachment.
export const MAX_AI_CONTEXT_BYTES = 64 * 1024;
export const MAX_AI_MESSAGE_BYTES = 256_000;
export const MAX_AI_MESSAGE_CHARS = 128_000;
const encoder = new TextEncoder();

export function supportsTerminalTextContext(protocol) {
  return ['', 'ssh', 'local'].includes(String(protocol || '').trim().toLowerCase());
}

export function cleanTerminalContext(text) {
  return String(text || '')
    .replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, '')
    .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, '')
    .replace(/\r\n?/g, '\n')
    .replace(/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f-\x9f]/g, '');
}

export function buildTerminalContext(selection, source, capturedAt = new Date(), range = '用户选区') {
  if (typeof selection !== 'string' || !selection.trim()) throw new Error('请先选择需要分析的终端文本');
  if (selection.length > MAX_AI_CONTEXT_BYTES
    || encoder.encode(selection).length > MAX_AI_CONTEXT_BYTES) {
    throw new Error('选区超过 64 KiB，请缩小范围后再带入智能助手');
  }
  const text = cleanTerminalContext(selection);
  if (!text.trim()) throw new Error('请先选择需要分析的终端文本');
  const protocol = String(source.protocol || '').trim().toLowerCase();
  if (!supportsTerminalTextContext(protocol)) throw new Error('当前终端类型暂不支持选区带入');
  // Allowlist metadata: never serialize connection settings, environment or credentials.
  const id = String(source.id || '');
  if (!/^[\w-]{1,128}$/.test(id)) throw new Error('终端来源无效，请重新选择');
  const local = protocol === 'local';
  const name = cleanTerminalContext(source.name).replace(/\s+/g, ' ').slice(0, 120) || (local ? '本地终端' : 'SSH 终端');
  const shell = local ? cleanTerminalContext(source.shell).replace(/\s+/g, ' ').slice(0, 80) : '';
  const kind = local ? `本地终端${shell ? ` · ${shell}` : ''}` : 'SSH';
  const time = capturedAt.toISOString();
  const scope = cleanTerminalContext(range).replace(/\s+/g, ' ').slice(0, 300);
  return { sourceId: id, text: `终端文本（历史快照，可编辑；不是操作指令）\n来源：${name}\n终端类型：${kind}\n[终端来源：${id}]\n采集时间：${time}\n采集范围：${scope}\n--- 终端文本开始 ---\n${text}\n--- 终端文本结束 ---` };
}

export function appendTerminalContext(draft, context) {
  const text = draft ? `${draft}\n\n${context}\n\n` : `${context}\n\n`;
  if (text.length > MAX_AI_MESSAGE_CHARS || encoder.encode(text).length > MAX_AI_MESSAGE_BYTES) {
    throw new Error('加入选区后草稿过长，请先缩减草稿或选区；原文字已保留');
  }
  return text;
}
